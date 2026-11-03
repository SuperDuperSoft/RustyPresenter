# FEAT-1 — Themes, Backgrounds and Overlays: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Named themes with a starred church default and item > service > default resolution, rich text on announcements and text overlays, a background library with gradients and a Darken value, and a generic overlay layer (text signs, pictures, clocks) that stays up while slides change. Every type, rule and drawing function is built so FEAT-2, FEAT-4 and FEAT-7 can plug into it.

**Architecture:** The plan has two parts, because Plan 3 (`native-operator`) and Plan 4 (`native-imports-recovery-packaging`) are rewriting the shared files right now. **Part A** builds all of FEAT-1's logic in **new files only**, under one new module tree `src/theme/` (plus `src/strings/themes.rs`, which `src/theme/mod.rs` includes with a `#[path]` attribute until Part B). It contains final data types with serde defaults, pure resolvers and state machines, and renderers that take explicit inputs. Types that the spec places on existing structs (`model::Theme`, `model::Background`, `Frame.overlays`, `Command::Overlay`, …) are defined here as standalone types and functions. Part A changes exactly one existing file, `src/lib.rs`, with one line: `pub mod theme;`. **Part B** is blocked until `native-foundation` has Plans 3 and 4 merged. It first moves the files the spec names (`src/overlay.rs`, `src/richtext.rs`, `src/backgrounds.rs`, `src/render/overlay.rs`, `src/render/background.rs`) to their spec paths with `git mv`, then wires Part A into `model`, `store`, `slides`, `live`, `render`, `room`, `app`, Plan 3's operator UI and the phone remote: old definitions become re-exports of the Part A types, and the existing functions call the Part A functions.

**Tech Stack:** Rust 2024; existing crates only: `serde`/`serde_json`, `uuid` 1 (`v4`, `serde`), `chrono` 0.4 (`clock` only, no `serde` feature, so RFC 3339 goes through a small `with` module), `cosmic-text` 0.19 (`Buffer::set_rich_text`, `Attrs::metadata`), `tiny-skia` 0.12 (`LinearGradient`, `Pixmap`), `image` 0.25 (`RgbaImage`). Seven new font files, all OFL: Noto Sans Bold and Italic (required, Task A8) and Noto Serif Regular/Bold, Montserrat SemiBold/Bold and Oswald Medium (spec decision 21 default, Task A8b). **No `Cargo.toml` change.** Part B uses Slint 1.18 and Plan 3's glue as it is when that work lands.

**Spec:** `docs/superpowers/specs/2026-11-01-feat1-themes-backgrounds-props-design.md` (FEAT-1). Also: `2026-10-29-rustypresenter-v1-design.md` §6.2–6.3 (clear matrix, incoming slides), `2026-10-29-native-slint-design.md` §4 (fonts), Plan 3 (`docs/superpowers/plans/2026-11-01-native-operator.md`, Tasks 6, 9, 10, 18, 19, 20) and Plan 4 (`2026-11-01-native-imports-recovery-packaging.md`, Tasks 8, 9, 12, 13) for the Part B file layout. FEAT-2 §5.11 (phone allowlist) and FEAT-4 (reconcile, `LookOnScreen`) are referenced by Part B only.

## Global Constraints

- **Split rule (Part A).** Create new files only. The one exception is `src/lib.rs`, which gets exactly one line, `pub mod theme;` (Task A1). Part A does **not** create `src/strings/mod.rs` and does not add `pub mod strings;` to `src/lib.rs`: FEAT-1 through FEAT-7 all add `src/strings/<area>.rs` on parallel branches, and two branches creating `src/strings/mod.rs` would be a guaranteed add/add conflict. Instead `src/theme/mod.rs` declares `#[path = "../strings/themes.rs"] pub mod strings;` (the FEAT-2/3/5 pattern), and Part A code refers to the strings as `crate::theme::strings::…`. Part B Task B1 adds `pub mod themes;` to the shared `src/strings/mod.rs` (creating it, and `pub mod strings;` in `src/lib.rs`, only if no other plan has) and replaces the `#[path]` line with `pub use crate::strings::themes as strings;`. Part A must not edit `src/app.rs`, `src/live.rs`, `src/model.rs`, `src/store.rs`, `src/room.rs`, `src/slides.rs`, `src/server.rs`, `src/render/*`, `src/ui/**`, `ui/**`, `web/**`, `src/bible*` or `Cargo.toml`. Part A may *read* existing public items, using these exact paths (only what `render/mod.rs` re-exports is reachable as `crate::render::X`):
  - `crate::model::{AssetId, MediaStatus, MediaFile, MediaKind, Settings, Service}`
  - `crate::render::{Fonts, Rect, Rendered, parse_color}`
  - `crate::render::layout::{BUNDLED_FAMILY, CANVAS_W, clamp_canvas}` (these are `pub` in `layout` but **not** re-exported from `render/mod.rs`)
  - `Fonts.sys` is `pub(crate)` and visible inside the crate. `render::fit_steps` is **not** used by Part A (its 26 px floor is wrong for overlays; see Task A14).
- **TDD order in every Part A task.** Step 1 writes the failing tests **and** adds the task's `pub mod <name>;` line (in `src/theme/mod.rs`, `src/theme/paint/mod.rs`, `src/theme/overlay/mod.rs`, or for A1 `src/lib.rs`), so the new file is compiled from the start. The file holds only the `#[cfg(test)] mod tests` block (plus `use super::*;`) and no implementation yet. Step 2 therefore really fails, with `error[E0412]: cannot find type …` / `error[E0425]: cannot find function …` naming the items the task must add. "0 tests run" in Step 2 means the `pub mod` line is missing: add it before going on. Step 3 adds the implementation above the test block.
- **Part A test command** (PowerShell; `$fc` stands for the full path):
  `$fc = "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd"`
  `& $fc test --release --lib <module path>`. If that helper is gone, `cargo test --release --lib <module path>` from the worktree is equivalent. Every Part A task also ends with `& $fc test --release --lib` (whole library), which must stay green.
- **Final paths after Part B (decided: the files move).** FEAT-2's plan edits `src/overlay.rs` (new `OverlaySource` arms) and `src/render/overlay.rs` (new `draw` arms), and a `pub use` re-export cannot receive new enum arms. So Part B Task B1 **physically moves** these files to the spec §4/§6 paths with `git mv`:

  | Part A file | Final file (after B1) | Old path kept as |
  |---|---|---|
  | `src/theme/overlay/mod.rs` | `src/overlay.rs` | `pub use crate::overlay;` in `src/theme/mod.rs` |
  | `src/theme/overlay/slots.rs` | `src/overlay/slots.rs` | (via `crate::theme::overlay::slots`) |
  | `src/theme/richtext.rs` | `src/richtext.rs` | `pub use crate::richtext;` |
  | `src/theme/backgrounds.rs` | `src/backgrounds.rs` | `pub use crate::backgrounds;` |
  | `src/theme/paint/overlay.rs` | `src/render/overlay.rs` | `pub use crate::render::overlay;` in `src/theme/paint/mod.rs` |
  | `src/theme/paint/background.rs` | `src/render/background.rs` | `pub use crate::render::background;` in `src/theme/paint/mod.rs` |
  | `src/strings/themes.rs` (via `#[path]`) | unchanged file, now `pub mod themes;` in `src/strings/mod.rs` | `pub use crate::strings::themes as strings;` in `src/theme/mod.rs` |

  Every other Part A file stays in `src/theme/`. So that the move needs no code edits, the six moving files refer to other FEAT-1 items only by absolute paths (`crate::theme::richtext::…`, `crate::theme::strings::…`, `crate::theme::paint::runs::…`), never `super::…` outside their own `#[cfg(test)]` block. `model::{Theme, TextAlign, Background}` become re-exports of `crate::theme::{Theme, TextAlign}` and `crate::backgrounds::Background`. Because the old paths stay as re-exports, Part A code and tests that say `crate::theme::…` keep compiling unchanged after the move; new Part B code uses the final paths. FEAT-2 Part B and FEAT-7 Part B therefore find `src/overlay.rs`, `src/render/overlay.rs`, `theme::find`, `theme::church_default`, `theme::builtin` and `theme::resolve` where their plans expect them.
- **Serde compatibility.** Every new field has `#[serde(default)]`, so old `data.json` and `live-state.json` load unchanged. This includes fields whose *type* is `#[serde(default)]` (`OverlayLook`, `Placement`): a container-level `#[serde(default)]` only fills missing fields *inside* the struct, not the struct field being absent from its parent, so the parent field needs its own `#[serde(default)]` too. Optional or empty new fields use `skip_serializing_if`, so data without the new features serializes exactly as it did before. Every persisted enum that may grow (`OverlaySource`) has `#[serde(other)] Unknown`.
- **Format number.** `store::SUPPORTED_FORMAT` (Part B Task B2) is the **only** format constant (spec §4.7). Part A defines no format constant; `theme::migrate::migrate_1_to_2` writes the literal `2` as its own step target.
- **`BoundCtx` is built only through its constructor** `BoundCtx::at(now)` (Task A6), never with a struct literal, in FEAT-1 code and tests. FEAT-2 adds a `timers` field and gives it a default in `at`, so no FEAT-1 line breaks.
- **Time:** `chrono` only (never `jiff`). Persisted instants are `chrono::DateTime<Utc>` as RFC 3339 through `theme::backgrounds::rfc3339` (`with` module, because the chrono `serde` feature is off). The clock overlay uses `chrono::DateTime<Local>` at runtime only.
- **Words.** UI and identifiers say "theme", "background", "overlay" and "screen view". Never "Look", "Props", "layer" or "z-order" in user text, and no new identifiers containing `look`/`prop` **except** the spec-fixed `OverlayLook`/`look` (spec §14 open note; FEAT-2 shares them).
- **Strings.** Every user-visible core message is a `pub const` or `pub fn` in `src/strings/themes.rs` (Task A1; reached as `crate::theme::strings` in Part A), copied verbatim from spec §3 and §8. Code uses the constants, never literals: this includes error texts (`EXPECTED_OBJECT`, `ITEM_TWICE`) and generated names (`numbered_background(n)`).
- **Hashing.** `Theme`, `Background`, `RunStyle`, `StyleSpan`, `OverlaySource`, `Placement` and `OverlayLook` derive `Eq + Hash`, because they are part of render cache keys.
- **Defaults give today's pixels.** `Theme::default()` with the new fields at their defaults must render exactly as `model::Theme::default()` does today (checked by the existing goldens in Part B Task B5).
- **Commits** (when executing): one per task, message as given, ending with the attribution line the harness supplies.

### Plan decisions (the spec leaves these open; defaults chosen)

1. **One module tree** `src/theme/` keeps Part A to one `lib.rs` line. Part B Task B1 moves the six files the spec (and FEAT-2) name to their final paths with `git mv` and leaves re-exports at the old paths (Global Constraints, "Final paths after Part B").
2. **Merge by key works on JSON values** (`theme::merge`), not on hand-written `Option<Option<T>>` mirror structs. One generic function covers every present and future `Service`/`ServiceItem`/`Settings` field from every FEAT spec. A key that is present replaces the whole top-level field (shallow merge). `service/update` items are merged one by one by `id`.
3. **`theme::paint::overlay::draw` (final path `render::overlay::draw`) takes `&mut Fonts`** (the spec writes `&Fonts`; shaping needs `&mut`, as `render_text` already does).
4. **Built-in colour names:** Black `#000000`, Night blue `#0b1a33`, Dark green `#1d2b1f`, Plum `#2b1530`, Brown `#3a2412`, White `#ffffff`.
5. **Clock text:** 12-hour is `9:05 AM` (`%-I:%M %p`, with seconds `%-I:%M:%S %p`). 24-hour is `21:05` (`%H:%M`, `%H:%M:%S`).
6. **Overlay defaults:** width Picture 12 %, Clock 20 %, Text 60 %; margin 4; anchors as spec §3.6 (BottomRight for pictures and clocks, Bottom for text). A `Behind` box has a 10-logical-px radius. A `Bar` sits flush on its edge (no margin) and spans the full width.
7. **Overlay look clamps:** size 20–120, weight snapped to 400/600/700, `box_opacity` ≤ 100, `margin` ≤ 20, `opacity` ≤ 100.
8. **Gradient angle** uses CSS `linear-gradient` geometry (180° = top→bottom, 90° = left→right, 135° = top-left→bottom-right), taken mod 360.
9. **`resolve` for deck and media items** skips `item.theme`. Resolution starts at the service theme, and `item.background` still wins.
10. **Extra font families are bundled** (spec decision 21 default): Noto Serif Regular/Bold, Montserrat SemiBold/Bold, Oswald Medium, about 2 MB, listed after Noto Sans in `BUNDLED_FAMILIES` and marked "works on every computer" in the theme editor (Task A8b, used by B8).
11. **Overlay text fit** uses its own step list, `overlay_fit_steps(max)`: −4 px above 60 px, else −2 px, floor `MIN_OVERLAY_PX` = 18, and a start size below 18 is not raised (an overlay at size 20 draws at 20). Slide text keeps `render::fit_steps` with its 26 px floor.
12. **Delete-theme count sentence** (spec §3.1 step 6 only gives the plural form). Each count is named only when it is not 0, with singular nouns for 1, and the verb agrees: "1 item uses it", "2 items use it", "1 service uses it", "1 item and 1 service use it", "3 items and 2 services use it". Then "; they'll use the church default instead."
13. **`validate_save` refuses `STANDARD_ID`** with `STANDARD_SAVE_ELSEWHERE` ("Standard is changed with Save on its own editor."). Standard is only written by `theme/save-standard`, which stores the style in `Settings.theme` and `Settings.background`, never in `Data.themes`.
14. **Uppercase changes characters, not necessarily bytes.** The spec's "ß becomes SS" changes the character count (1 → 2) but not the byte count (`ß` is 2 bytes, `SS` is 2 bytes), so spans do not move. Spans move only when an uppercase form has a different UTF-8 length, e.g. the ligature `ﬁ` (3 bytes) → `FI` (2 bytes). Task A4 tests both.
15. **Newer data and parse errors** (spec §4.7). `migrate::read_format` parses only the `{"format"}` header from syntactically valid JSON. A JSON **syntax** error is `Err` and goes to the existing "could not be read" path, the same as today. A parseable header with `format > SUPPORTED_FORMAT` whose full typed parse then fails opens **read-only with empty data**.

## Review Focus

1. **Multi-byte text in rich text.** "Café ß 🙏" with spans whose offsets fall inside a character (hand-edited file, or a stale span after an edit). Expected: no panic; `normalize` snaps offsets to char boundaries; `uppercase` leaves spans in place when "ß" becomes "SS" (same byte length) and shifts them when "ﬁ" becomes "FI" (3 → 2 bytes). Tests in Tasks A4 (`normalize_snaps_to_char_boundaries`, `uppercase_keeps_spans_aligned_for_eszett`, `uppercase_shifts_spans_when_byte_length_changes`) and A5 (`edit_with_emoji_keeps_boundaries`).
2. **Hand-edited or restored data.** Background JSON from v0.2 (`{"type":"image","asset":"a.png"}`), a gradient with `angle: 400`, a saved theme whose name is spaces, two saved themes that differ only in case, and a saved theme that overrides a built-in id. Expected: everything loads; angle is used mod 360; save rejects the bad names with the spec messages. Tests in Tasks A2 and A3.
3. **Odd pictures and screens for overlays.** A 1×4000 px logo, a 0×0 canvas request, a 4:3 1024×768 canvas, an overlay wider than the screen (width 100 + margin). Expected: the placed box always lies inside the canvas and nothing panics. Test in Task A14 (`overlay_never_leaves_canvas`).
4. **Merge patches that don't match the stored data.** `service/update` with an unknown item id, two items with the same id, a changed `kind`, or a body that isn't an object. Expected: unknown ids are added as sent, a duplicate id is an error, a changed kind replaces the item, and a non-object is `EXPECTED_OBJECT` ("Expected an object."). Test in Task A9.
5. **Clock edge times.** Midnight, noon, and 23:59:59 with seconds on. Expected: `12:00 AM`, `12:00 PM`, `23:59:59` (24-hour). Test in Task A6.

---

# PART A — Core, buildable now

Each task here builds and tests on the current `feat-roadmap` tree. None of them touches a file that Plan 3 or Plan 4 is rewriting.

### Task A1: Module scaffold, strings and the extended text style

**Files:**
- Create: `src/theme/mod.rs`, `src/theme/style.rs`, `src/strings/themes.rs` (do **not** create `src/strings/mod.rs`; see Global Constraints)
- Modify: `src/lib.rs` (add `pub mod theme;`, the only `lib.rs` line in Part A)

**Interfaces:**
- Produces: `crate::theme::style::{Theme, TextAlign, clamp_style}`, re-exported as `crate::theme::{Theme, TextAlign}`.
  ```rust
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
  #[serde(default)]
  pub struct Theme {
      pub font: String, pub size: u32, pub color: String, pub shadow: bool,
      pub position: String,            // "center" | "lower" | "top"
      pub uppercase: bool, pub scripture_size: u32,
      pub weight: u16,                 // 400 | 600 | 700, default 600
      pub align: TextAlign,            // default Center
      pub outline: u8,                 // 0 | 3 | 6 (≤ 12), default 0
      pub outline_color: String,       // default "#000000"
      pub band: bool,                  // default false
      pub band_color: String,          // default "#000000"
      pub band_opacity: u8,            // 0–100, default 55
  }
  #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
  #[serde(rename_all = "snake_case")]
  pub enum TextAlign { Left, #[default] Center, Right }
  pub fn clamp_style(t: &mut Theme); // size & scripture_size 24–200; weight → nearest of 400/600/700 (<500→400, <650→600, else 700); outline ≤ 12; band_opacity ≤ 100; position not in {center, lower, top} → "center"
  ```
  The existing fields keep today's defaults: `"Segoe UI"`, 84, `"#ffffff"`, true, `"center"`, false, 64.
- Produces: `crate::theme::strings` (file `src/strings/themes.rs`, declared in `src/theme/mod.rs` as `#[path = "../strings/themes.rs"] pub mod strings;`; it becomes `crate::strings::themes` in Part B Task B1). Constants (exact text from spec §3/§8): `THEME_NAME_REQUIRED`, `THEME_NAME_RESERVED`, `THEME_GONE`, `BUILTIN_NO_DELETE`, `BACKGROUND_KIND`, `BACKGROUND_NAME_REQUIRED`, `BACKGROUND_GONE`, `OVERLAY_NAME_REQUIRED`, `OVERLAY_TEXT_REQUIRED`, `OVERLAY_GONE`, `OVERLAY_KIND`, `OVERLAY_CAP`, `LOGO_FIRST`, `PICTURE_GONE`, `PICTURE_UNREADABLE` (`"the picture could not be loaded."`), `NEWER_DATA`, `SCREEN_CLEARED`, `OVERLAY_CHANGED_TIP`, `THEME_SAVED`, `THEME_DELETED`, `LOGO_SAVED`, `LOGO_REMOVED`, `ADDED_TO_BACKGROUNDS`, `STANDARD_NAME` ("Standard"), `CHURCH_DEFAULT` ("Church default"), `STANDARD_HINT`, `DEFAULT_HINT`, `NEW_THEME_NAME`, `OWN_BACKGROUND_SUFFIX` (" · own background"), `FROM_THEME` ("From theme"), `STANDARD_BACKGROUND` ("Standard background"), `CURRENT_NOT_IN_LIBRARY` ("Current (not in your backgrounds)"), `COULDNT_CONVERT` ("Couldn't convert"), `BOLD_DISABLED_TIP` ("Text is already bold in this theme"), `DARKEN_HELP` ("Makes text easier to read"). `OVERLAY_KIND` = "Only text, picture and clock overlays can be saved." (the spec gives no wording; this is the plan's). Also, all used in place of literals elsewhere in Part A:
  - `EXPECTED_OBJECT` = "Expected an object." and `ITEM_TWICE` = "The same item is listed twice." (Task A9);
  - `STANDARD_SAVE_ELSEWHERE` = "Standard is changed with Save on its own editor." (plan decision 13, Task A3);
  - `TEXT_OVERLAY_HELP` = "For a sign that stays up." and `TEXT_OVERLAY_HELP_FEAT2` = "For a sign that stays up. For a notice like a parent's name, use Message (once available)." (spec §3.6 step 3; Part B Task B12 shows the second only once FEAT-2 has merged);
  - `BUNDLED_FONT_NOTE` = "works on every computer" (spec §3.1 step 3; Task B8).
- Functions: `theme_exists(name) -> String` (“There's already a theme called “<name>”.”), `overlay_failed(name, reason) -> String`, `own_backgrounds(n) -> String` ("1 item has its own background. …" / "N items have their own background. Use the theme's background for them too?"), `font_missing(font)`, `used_by(n)` ("Used by N items", "Used by 1 item"), `numbered_background(n)` ("Background 1", Task A10), `make_default_question(name)`, `now_default(name)`, `delete_theme_question(name, items, services, is_default)` (spec §3.1 step 6 with plan decision 12: each count is named only when non-zero, singular for 1, verb agreeing; the counts sentence is left out when both are 0; the default sentence is added when `is_default`), `remove_background_question(name)`, `used_by_screen_views(names: &[String])` ("Used by screen view(s): a, b"), `converting(percent)` ("Converting 40 %"), `theme_chip(name)` ("Theme: <name>"), `range_chip(style: &RunStyle, text: &str)`. `range_chip` lists the parts in the order Bold, Italic, Underline, colour (`#f5b94a` → "Gold", `#ff4f55` → "Red", other → "Color"), size (75 Small, 125 Large, 150 Huge), joined with ", ", then `: ‘<text>’`. It is implemented in Task A5 (it needs `RunStyle`); declare it there.

- [ ] **Step 1: Write the failing tests and the module lines.** Add `pub mod theme;` to `src/lib.rs`. Create `src/theme/mod.rs` with exactly:

```rust
//! FEAT-1: themes, backgrounds, rich text and overlays (core, Part A).
#[path = "../strings/themes.rs"]
pub mod strings;
pub mod style;
```

Create `src/theme/style.rs` and `src/strings/themes.rs` each holding only a `#[cfg(test)] mod tests { use super::*; … }` block with these tests (no implementation yet):

```rust
#[test]
fn old_theme_json_loads_with_todays_defaults() {
    let t: Theme = serde_json::from_str(r##"{"font":"Segoe UI","size":84,"color":"#ffffff","shadow":true,"position":"center","uppercase":false,"scripture_size":64}"##).unwrap();
    assert_eq!(t, Theme::default());
    assert_eq!((t.weight, t.align, t.outline, t.band, t.band_opacity), (600, TextAlign::Center, 0, false, 55));
    assert_eq!((t.outline_color.as_str(), t.band_color.as_str()), ("#000000", "#000000"));
}
#[test]
fn theme_round_trips() { let t = Theme { align: TextAlign::Right, band: true, ..Theme::default() }; assert_eq!(serde_json::from_value::<Theme>(serde_json::to_value(&t).unwrap()).unwrap(), t); }
#[test]
fn clamp_style_snaps_weight_and_limits() {
    let mut t = Theme { size: 500, scripture_size: 1, weight: 550, outline: 40, band_opacity: 200, position: "middle".into(), ..Theme::default() };
    clamp_style(&mut t);
    assert_eq!((t.size, t.scripture_size, t.weight, t.outline, t.band_opacity, t.position.as_str()), (200, 24, 600, 12, 100, "center"));
}
// strings
#[test]
fn delete_question_leaves_out_zero_counts_and_adds_default_line() {
    assert_eq!(delete_theme_question("Christmas", 0, 0, false), "Delete the theme “Christmas”?");
    assert_eq!(delete_theme_question("Christmas", 3, 1, true), "Delete the theme “Christmas”? 3 items and 1 service use it; they'll use the church default instead. It's the church default, so Standard becomes the church default.");
}
#[test]
fn delete_question_singular_and_one_sided_counts() {
    assert_eq!(delete_theme_question("A", 1, 0, false), "Delete the theme “A”? 1 item uses it; they'll use the church default instead.");
    assert_eq!(delete_theme_question("A", 2, 0, false), "Delete the theme “A”? 2 items use it; they'll use the church default instead.");
    assert_eq!(delete_theme_question("A", 0, 1, false), "Delete the theme “A”? 1 service uses it; they'll use the church default instead.");
    assert_eq!(delete_theme_question("A", 1, 1, false), "Delete the theme “A”? 1 item and 1 service use it; they'll use the church default instead.");
    assert_eq!(delete_theme_question("A", 0, 0, true), "Delete the theme “A”? It's the church default, so Standard becomes the church default.");
}
#[test]
fn formatted_strings_match_spec() {
    assert_eq!(theme_exists("Easter"), "There's already a theme called “Easter”.");
    assert_eq!(overlay_failed("Logo", PICTURE_UNREADABLE), "Couldn't show the overlay “Logo”: the picture could not be loaded.");
    assert_eq!(font_missing("Lato"), "“Lato” isn't installed on this computer, so slides use Noto Sans.");
    assert_eq!(converting(40), "Converting 40 %");
    assert_eq!((numbered_background(1), numbered_background(12)), ("Background 1".to_string(), "Background 12".to_string()));
    assert_eq!((used_by(1), used_by(3)), ("Used by 1 item".to_string(), "Used by 3 items".to_string()));
}
#[test]
fn text_overlay_help_lines_match_spec() {
    assert!(TEXT_OVERLAY_HELP_FEAT2.starts_with(TEXT_OVERLAY_HELP));
    assert!(TEXT_OVERLAY_HELP_FEAT2.ends_with("use Message (once available)."));
}
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::style` and `& $fc test --release --lib theme::strings`. Expected: FAIL to compile with `error[E0412]: cannot find type `Theme`` / `TextAlign` and `error[E0425]: cannot find function `clamp_style`` / `delete_theme_question` / `theme_exists` … (and `cannot find value` for the constants).
- [ ] **Step 3: Implement** the items listed under Interfaces above the test blocks. Add `pub use style::{Theme, TextAlign, clamp_style};` to `src/theme/mod.rs`. Later tasks add their own `pub mod` lines to it.
- [ ] **Step 4: Run** both commands, then `& $fc test --release --lib`. Expected: PASS (6 tests in `theme::strings`, 3 in `theme::style`), and no existing test changes.
- [ ] **Step 5: Commit** `feat(theme): module scaffold, strings and extended text style (FEAT-1 A1)`.

### Task A2: Background value, library entries and built-ins

**Files:**
- Create: `src/theme/backgrounds.rs`
- Modify: `src/theme/mod.rs` (`pub mod backgrounds;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum Background {
      Color { color: String },
      Image { asset: AssetId, #[serde(default, skip_serializing_if = "is_zero")] dim: u8 },
      Video { asset: AssetId, #[serde(default, skip_serializing_if = "is_zero")] dim: u8 },
      Gradient { from: String, to: String, #[serde(default)] angle: u16, #[serde(default, skip_serializing_if = "is_zero")] dim: u8 },
  }
  pub const MAX_DIM: u8 = 80;
  impl Background {
      pub fn dim(&self) -> u8;                       // Color → 0
      pub fn with_dim(self, dim: u8) -> Self;        // clamps to MAX_DIM; Color unchanged
      pub fn asset(&self) -> Option<&AssetId>;
      pub fn same_source(&self, other: &Background) -> bool; // equal ignoring dim
      pub fn angle_deg(&self) -> u16;                // Gradient: angle % 360, else 0
  }
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
  pub struct BackgroundEntry {
      pub id: Uuid, pub name: String, pub background: Background,
      #[serde(default = "ready")] pub status: MediaStatus,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub thumb: Option<AssetId>,
      #[serde(with = "rfc3339")] pub added: chrono::DateTime<Utc>,
  }
  impl BackgroundEntry { pub fn new(name: &str, background: Background, status: MediaStatus, now: DateTime<Utc>) -> Self; pub fn pickable(&self) -> bool /* status == Ready */; }
  pub fn builtin_backgrounds() -> Vec<(&'static str, Background)>;
  pub fn file_stem_name(file_name: &str) -> String; // "Easter Cross.final.jpg" → "Easter Cross.final"; "" → "Background"
  pub mod rfc3339 { pub fn serialize<S>(…); pub fn deserialize<'de, D>(…) -> Result<DateTime<Utc>, D::Error>; } // to_rfc3339_opts(SecondsFormat::Secs, true) / parse_from_rfc3339
  ```
  `MediaStatus` and `AssetId` come from `crate::model`.

- [ ] **Step 1: Write the failing tests and add `pub mod backgrounds;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order):**

```rust
#[test]
fn v02_background_json_loads_with_zero_dim_and_reserializes_identically() {
    for j in [r##"{"type":"color","color":"#000000"}"##, r#"{"type":"image","asset":"ab.png"}"#, r#"{"type":"video","asset":"cd.mp4"}"#] {
        let b: Background = serde_json::from_str(j).unwrap();
        assert_eq!(b.dim(), 0);
        assert_eq!(serde_json::to_string(&b).unwrap(), j);
    }
}
#[test]
fn gradient_round_trips_and_angle_wraps() {
    let g = Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 400, dim: 30 };
    assert_eq!(serde_json::from_value::<Background>(serde_json::to_value(&g).unwrap()).unwrap(), g);
    assert_eq!(g.angle_deg(), 40);
}
#[test]
fn with_dim_clamps_to_80_and_ignores_colors() { /* Image dim 95 → 80; Color unchanged */ }
#[test]
fn same_source_ignores_dim() { /* Image{a,0} vs Image{a,50} → true; Image{a} vs Video{a} → false */ }
#[test]
fn builtin_backgrounds_match_plan3_swatches_and_six_gradients() {
    let b = builtin_backgrounds();
    let colors: Vec<_> = b.iter().filter_map(|(_, x)| match x { Background::Color { color } => Some(color.as_str()), _ => None }).collect();
    assert_eq!(colors, ["#000000", "#0b1a33", "#1d2b1f", "#2b1530", "#3a2412", "#ffffff"]);
    let grads: Vec<_> = b.iter().filter(|(_, x)| matches!(x, Background::Gradient { .. })).map(|(n, _)| *n).collect();
    assert_eq!(grads, ["Midnight", "Deep purple", "Forest", "Ember", "Ocean", "Slate"]);
    assert!(b.contains(&("Ember", Background::Gradient { from: "#3a2412".into(), to: "#0d0806".into(), angle: 135, dim: 0 })));
}
#[test]
fn background_entry_added_is_rfc3339_and_status_defaults_ready() {
    let e = BackgroundEntry::new("Cross", Background::Image { asset: "a.png".into(), dim: 0 }, MediaStatus::Ready, Utc.with_ymd_and_hms(2026, 11, 1, 9, 30, 0).unwrap());
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["added"], "2026-11-01T09:30:00Z");
    let mut v2 = v.clone(); v2.as_object_mut().unwrap().remove("status");
    assert_eq!(serde_json::from_value::<BackgroundEntry>(v2).unwrap().status, MediaStatus::Ready);
}
#[test]
fn converting_entry_is_not_pickable() { /* status Converting{percent:40} → pickable() false */ }
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::backgrounds`. Expected: FAIL to compile with `cannot find type/function` errors naming `Background`, `BackgroundEntry`, `MAX_DIM`, `builtin_backgrounds` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.** Copy the built-in values from spec §4.3 exactly, and the colour names from plan decision 4.
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): background value with gradient and dim, library entries, built-ins (FEAT-1 A2)`.

### Task A3: Saved themes, the church default and resolution

**Files:**
- Create: `src/theme/saved.rs`
- Modify: `src/theme/mod.rs` (`pub mod saved; pub use saved::*;` so the spec's `theme::ThemeId`, `theme::resolve`… paths exist)

**Interfaces:**
- Consumes: `Theme`, `clamp_style` (A1); `Background` (A2); `crate::theme::strings` (A1).
- Produces:
  ```rust
  pub type ThemeId = Uuid;
  pub const STANDARD_ID: ThemeId = Uuid::from_u128(0x00000000_0000_4000_8000_000000000101);
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
  pub struct SavedTheme { pub id: ThemeId, pub name: String, pub style: Theme,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub background: Option<Background> }
  pub fn builtin() -> Vec<SavedTheme>;                     // empty in FEAT-1
  pub struct Catalog<'a> { pub standard_style: &'a Theme, pub standard_background: &'a Background,
      pub saved: &'a [SavedTheme], pub default_theme: Option<ThemeId>, pub builtins: Vec<SavedTheme> }
  impl<'a> Catalog<'a> {
      pub fn new(standard_style: &'a Theme, standard_background: &'a Background, saved: &'a [SavedTheme], default_theme: Option<ThemeId>) -> Self; // builtins = builtin()
      pub fn with_builtins(self, b: Vec<SavedTheme>) -> Self;   // tests and FEAT-7
      pub fn standard(&self) -> SavedTheme;                     // STANDARD_ID, "Standard", standard_style, background None
      pub fn find(&self, id: ThemeId) -> Option<SavedTheme>;    // Standard, then saved (an override wins), then builtins
      pub fn church_default(&self) -> SavedTheme;               // find(default_theme) or standard()
      pub fn resolve(&self, service_theme: Option<ThemeId>, item: ItemThemeRef<'_>) -> Resolved;
      pub fn rows(&self) -> Vec<ThemeRow>;                      // Standard, builtins (override shown if any), saved non-overrides by name (case-insensitive)
      pub fn name_taken(&self, name: &str, except: ThemeId) -> bool; // case-insensitive over saved + builtins + reserved
  }
  pub struct ItemThemeRef<'a> { pub theme: Option<ThemeId>, pub background: Option<&'a Background>, pub draws_text: bool }
  #[derive(Clone, Debug, PartialEq)] pub struct Resolved { pub style: Theme, pub background: Background, pub from: ThemeSource }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ThemeSource { Item(ThemeId), Service(ThemeId), Church(ThemeId) }
  #[derive(Clone, Debug, PartialEq)] pub struct ThemeRow { pub id: ThemeId, pub name: String, pub builtin: bool, pub overridden: bool, pub is_default: bool }
  pub fn validate_save(cat: &Catalog, t: SavedTheme) -> Result<SavedTheme, String>; // id == STANDARD_ID → STANDARD_SAVE_ELSEWHERE (plan decision 13); trim name; required; reserved; unique; nil id → new_v4; clamp_style
  pub fn default_ref(id: ThemeId) -> Option<ThemeId>;       // STANDARD_ID → None (theme/make-default)
  pub enum DeleteKind { Saved, ResetBuiltin }
  pub fn delete_kind(cat: &Catalog, id: ThemeId) -> Result<DeleteKind, String>; // Standard or builtin without override → BUILTIN_NO_DELETE; unknown → THEME_GONE
  pub fn copy_name(base: &str, taken: impl Fn(&str) -> bool) -> String; // "<base> copy", then "<base> copy 2", "… 3"
  ```
  **Resolution rule (spec §4.2).** The theme is the first of `item.theme` (only when `draws_text`) and `service_theme` that `find` resolves; otherwise `church_default`. The background is `item.background`, else the *chosen* theme's `background`, else `standard_background`. A dangling id is skipped. `from` names the level that supplied the theme.

- [ ] **Step 1: Write the failing tests and add `pub mod saved; pub use saved::*;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order)** (spec §9.1 `theme::tests` names; build a `Catalog` from local fixtures):

```rust
fn fx() -> (Theme, Background, Vec<SavedTheme>) { /* Standard: default theme + Color #000000; saved: "Christmas" (Bold, Gradient bg), "Youth" (no bg) */ }
#[test] fn item_theme_beats_service_and_default() { /* item=Youth, service=Christmas, default=Christmas → style = Youth, from = Item(youth) */ }
#[test] fn service_theme_beats_default() {}
#[test] fn dangling_item_theme_falls_through() { /* item = random id → Service */ }
#[test] fn theme_without_background_uses_standard_background_not_service_theme() {
    // service = Christmas (has gradient), item = Youth (no background) → background == standard Color #000000
}
#[test] fn item_background_beats_theme_background() {}
#[test] fn deck_and_media_items_ignore_theme() { /* draws_text false, item theme set → from = Service/Church */ }
#[test] fn starred_theme_is_church_default() {}
#[test] fn dangling_default_falls_back_to_standard() { /* church_default().id == STANDARD_ID */ }
#[test] fn builtin_theme_resolves_and_override_wins() {
    // with_builtins([B "Lower third"]); find(B) = builtin; with saved override (same id, name "Lower third", size 50) → find returns override; rows show overridden = true
}
#[test] fn rows_list_standard_first_then_builtins_then_saved_by_name() {}
#[test] fn validate_rejects_blank_reserved_and_case_duplicates() {
    assert_eq!(validate_save(&cat, named("   ")).unwrap_err(), THEME_NAME_REQUIRED);
    assert_eq!(validate_save(&cat, named("church DEFAULT")).unwrap_err(), THEME_NAME_RESERVED);
    assert_eq!(validate_save(&cat, named("christmas")).unwrap_err(), theme_exists("christmas"));
    assert!(validate_save(&cat, SavedTheme { id: christmas_id, name: "Christmas ".into(), ..christmas }).is_ok()); // own name is fine
}
#[test] fn validate_assigns_id_and_clamps() { /* nil id → non-nil; weight 900 → 700 */ }
#[test] fn validate_rejects_standard_id() {
    let t = SavedTheme { id: STANDARD_ID, name: "Mine".into(), style: Theme::default(), background: None };
    assert_eq!(validate_save(&cat, t).unwrap_err(), STANDARD_SAVE_ELSEWHERE);
}
#[test] fn delete_kind_refuses_standard_and_unedited_builtin() {}
#[test] fn copy_name_counts_up() { /* taken {"A copy","A copy 2"} → "A copy 3" */ }
#[test] fn make_default_keeps_previous_default_as_theme() {
    // default = Christmas → church_default is Christmas; default_ref(STANDARD_ID) = None → Standard again; Christmas still findable unchanged
}
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::saved`. Expected: FAIL to compile with `cannot find type/function` errors naming `SavedTheme`, `Catalog`, `STANDARD_ID`, `validate_save`, `delete_kind`, `copy_name` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.** `rows`/`name_taken` compare with `to_lowercase()`. Reserved names are `STANDARD_NAME` and `CHURCH_DEFAULT`. A saved entry whose id equals a built-in id is that built-in's override, so it is listed once (`overridden: true`) and never as a saved row.
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): saved themes, church default reference and resolution (FEAT-1 A3)`.

### Task A4: Rich text spans

**Files:**
- Create: `src/theme/richtext.rs`
- Modify: `src/theme/mod.rs` (`pub mod richtext;`)

**Interfaces:**
- Produces (byte offsets, `end` exclusive, spans sorted, non-overlapping, non-empty, never `RunStyle::default()`):
  ```rust
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
  pub struct RunStyle { /* bold, italic, underline: bool (skip if false); color: Option<String> (skip if None); size: u16 percent, 0 = 100 (skip if 0) */ }
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
  pub struct StyleSpan { pub start: u32, pub end: u32, #[serde(flatten)] pub style: RunStyle }
  pub const SIZES: [(u16, &str); 4] = [(75, "Small"), (0, "Normal"), (125, "Large"), (150, "Huge")];
  pub fn normalize(text: &str, spans: Vec<StyleSpan>) -> Vec<StyleSpan>; // snap start down / end up to char boundaries, clamp to len, drop empty + default-style, later span wins on overlap, sort, merge equal touching neighbours
  pub fn apply(spans: &[StyleSpan], range: Range<usize>, f: impl Fn(&mut RunStyle)) -> Vec<StyleSpan>; // gaps in range get default style before f
  pub fn clear(spans: &[StyleSpan], range: Range<usize>) -> Vec<StyleSpan>;
  pub fn slice(spans: &[StyleSpan], range: Range<usize>) -> Vec<StyleSpan>;  // cut to range, rebased to range.start
  pub fn blocks(body: &str, spans: &[StyleSpan]) -> Vec<(String, Vec<StyleSpan>)>; // slides::text_slides rule: "\r\n"→"\n" (spans remapped), split on "\n\n", trim, drop empty
  pub fn uppercase(text: &str, spans: &[StyleSpan]) -> (String, Vec<StyleSpan>);
  pub fn style_at(spans: &[StyleSpan], pos: usize) -> RunStyle;            // style of the byte before pos; pos 0 → default
  ```

- [ ] **Step 1: Write the failing tests and add `pub mod richtext;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order)** (spec §9.1 `richtext::tests`, plus Review Focus 1):

```rust
fn sp(s: u32, e: u32, f: impl Fn(&mut RunStyle)) -> StyleSpan { let mut st = RunStyle::default(); f(&mut st); StyleSpan { start: s, end: e, style: st } }
#[test] fn normalize_snaps_to_char_boundaries() {
    let t = "Café ß 🙏"; // é = 2 bytes at 3..5, 🙏 = 4 bytes at 9..13
    assert_eq!(normalize(t, vec![sp(4, 10, |s| s.bold = true)]), vec![sp(3, 13, |s| s.bold = true)]);
    assert_eq!(normalize(t, vec![sp(20, 30, |s| s.bold = true)]), vec![]);
}
#[test] fn normalize_merges_and_splits_overlaps() {
    // [0,4 bold] + [4,8 bold] → [0,8 bold]; [0,6 bold] then [3,9 italic] → [0,3 bold],[3,9 italic]
}
#[test] fn apply_bold_to_middle_splits_span() {
    // [0,10 italic], apply 3..6 bold → [0,3 i],[3,6 i+b],[6,10 i]
}
#[test] fn clear_formatting_removes_only_range() {}
#[test] fn slice_by_blank_line_blocks_rebases_offsets() {
    let body = "Sunday 10am\n\n  Pancake breakfast  ";
    let b = blocks(body, &[sp(0, 6, |s| s.bold = true), sp(15, 22, |s| s.color = Some("#f5b94a".into()))]);
    assert_eq!(b, vec![("Sunday 10am".into(), vec![sp(0, 6, |s| s.bold = true)]), ("Pancake breakfast".into(), vec![sp(0, 7, |s| s.color = Some("#f5b94a".into()))])]);
}
#[test] fn blocks_without_spans_match_text_slides() { /* "a\r\n\r\nb" → [("a",[]),("b",[])] */ }
// Plan decision 14: "ß → SS" changes characters (1 → 2), not bytes (2 → 2), so spans stay put.
#[test] fn uppercase_keeps_spans_aligned_for_eszett() {
    let (t, s) = uppercase("groß ja", &[sp(5, 7, |s| s.bold = true)]); // " j" is bytes 5..7
    assert_eq!(t, "GROSS JA"); assert_eq!(s, vec![sp(5, 7, |s| s.bold = true)]);
}
// "ﬁ" (U+FB01) is 3 bytes and uppercases to "FI" (2 bytes): spans after it move left by 1.
#[test] fn uppercase_shifts_spans_when_byte_length_changes() {
    let (t, s) = uppercase("ﬁne ja", &[sp(6, 8, |s| s.bold = true)]); // "ja" is bytes 6..8
    assert_eq!(t, "FINE JA"); assert_eq!(s, vec![sp(5, 7, |s| s.bold = true)]);
    let (_, s2) = uppercase("ﬁne ja", &[sp(0, 3, |s| s.italic = true)]); // span over the ligature itself
    assert_eq!(s2, vec![sp(0, 2, |s| s.italic = true)]);
}
#[test] fn style_at_reads_the_char_before() {}
#[test] fn spans_serialize_flat_and_skip_defaults() {
    assert_eq!(serde_json::to_string(&sp(0, 3, |s| s.bold = true)).unwrap(), r#"{"start":0,"end":3,"bold":true}"#);
}
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::richtext`. Expected: FAIL to compile with `cannot find type/function` errors naming `RunStyle`, `StyleSpan`, `normalize`, `apply`, `blocks`, `uppercase` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.** `normalize`: walk the spans in input order and paint them onto a list of boundary segments, so the last span wins; then rebuild the spans. `uppercase`: uppercase each segment between span boundaries separately (`str::to_uppercase`) and add up the new **byte** lengths, so a span boundary moves only when an earlier segment's UTF-8 length changed (plan decision 14).
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): rich text spans — normalize, apply, slice, blocks, uppercase (FEAT-1 A4)`.

### Task A5: Editing rich text (diff, adjust, ranges, the editor model)

**Files:**
- Create: `src/theme/rich_edit.rs`
- Modify: `src/theme/richtext.rs` (this plan's own file: add `adjust`, `diff_edit`, `ranges`), `src/theme/mod.rs` (`pub mod rich_edit;`), `src/strings/themes.rs` (`range_chip`, reached as `crate::theme::strings::range_chip`)

**Interfaces:**
- Consumes: A4.
- Produces in `richtext`:
  ```rust
  pub fn diff_edit(old: &str, new: &str) -> (usize, usize, usize);          // (start, removed, inserted), char-boundary safe
  pub fn adjust(spans: &[StyleSpan], start: usize, removed: usize, inserted: usize, inherit: Option<&RunStyle>) -> Vec<StyleSpan>; // inserted bytes get `inherit` (None = plain)
  pub fn ranges(text: &str, spans: &[StyleSpan]) -> Vec<(Range<usize>, RunStyle, String)>; // text cut at 24 chars + "…"
  ```
- Produces in `rich_edit` (pure model behind Part B's `RichEdit` widget):
  ```rust
  pub enum Toggle { Bold, Italic, Underline }
  #[derive(Clone, Debug, PartialEq)] pub struct RangeChip { pub label: String, pub start: usize, pub end: usize }
  pub struct RichEditModel { pub text: String, pub spans: Vec<StyleSpan>, /* private: pending: Option<(usize, RunStyle)> */ }
  impl RichEditModel {
      pub fn new(text: &str, spans: Vec<StyleSpan>) -> Self;                 // normalizes
      pub fn edited(&mut self, new_text: &str);                              // diff_edit + adjust(inherit = pending at start, else style_at) + normalize
      pub fn cursor_moved(&mut self, cursor: usize);                         // drops pending unless cursor == pending pos
      pub fn toggle(&mut self, sel: Range<usize>, t: Toggle);                // all-set → unset, else set; empty sel → pending
      pub fn set_color(&mut self, sel: Range<usize>, color: Option<String>); // None = "Theme color"
      pub fn set_size(&mut self, sel: Range<usize>, size: u16);
      pub fn clear(&mut self, sel: Range<usize>);
      pub fn pressed(&self, sel: Range<usize>) -> RunStyle;                  // pending if any, else style common to whole sel (style_at for empty sel)
      pub fn chips(&self) -> Vec<RangeChip>;                                 // crate::theme::strings::range_chip per range
      pub fn remove_chip(&mut self, i: usize);
      pub fn bold_disabled(theme_weight: u16) -> bool;                       // weight >= 700
  }
  ```

- [ ] **Step 1: Write the failing tests and add `pub mod rich_edit;` to `src/theme/mod.rs` (the `richtext` tests go into the existing `richtext` test block, the `range_chip` test into `theme::strings`), with no implementation yet (Global Constraints, TDD order):**

```rust
// richtext
#[test] fn diff_edit_finds_middle_insert() { assert_eq!(diff_edit("Sunday 10am", "Sunday at 10am"), (7, 0, 3)); }
#[test] fn typing_inside_span_extends_it() { /* [0,6 bold], insert 2 bytes at 3 with inherit = style_at(3) → [0,8 bold] */ }
#[test] fn typing_at_span_end_inherits_style() { /* [0,6 bold], insert at 6 with inherit = style_at(6) → [0,7 bold] */ }
#[test] fn delete_across_spans_shrinks_both() { /* [0,4 b],[4,8 i], delete 2..6 → [0,2 b],[2,4 i] */ }
#[test] fn ranges_label_cuts_long_text() {
    let t = "Pancake breakfast this Saturday morning";
    let r = ranges(t, &[sp(0, t.len() as u32, |s| s.bold = true)]);
    assert_eq!(r[0].2, "Pancake breakfast this S…");
}
// strings
#[test] fn range_chip_names_gold_and_large() {
    let st = RunStyle { color: Some("#f5b94a".into()), size: 125, ..Default::default() };
    assert_eq!(range_chip(&st, "Pancake breakfast"), "Gold, Large: ‘Pancake breakfast’");
}
// rich_edit
#[test] fn rich_edit_bold_selection_round_trips() { /* toggle Bold 0..6 then serialize spans → [{start:0,end:6,bold:true}] */ }
#[test] fn toggle_twice_removes() {}
#[test] fn rich_edit_pending_style_applies_to_typed_text() {
    let mut m = RichEditModel::new("Hi ", vec![]);
    m.toggle(3..3, Toggle::Bold);
    m.edited("Hi there");
    assert_eq!(m.spans, vec![sp(3, 8, |s| s.bold = true)]);
}
#[test] fn moving_cursor_drops_pending() {}
#[test] fn rich_edit_chip_x_clears_range() {}
#[test] fn pasted_text_takes_style_before_cursor() { /* spans [0,2 italic]; edited inserts "abc" at 2 → [0,5 italic] */ }
#[test] fn edit_with_emoji_keeps_boundaries() { /* "a🙏b" bold 1..5; replace emoji with "x" → spans valid for new text, no panic */ }
#[test] fn rich_edit_b_disabled_in_bold_theme() { assert!(RichEditModel::bold_disabled(700)); assert!(!RichEditModel::bold_disabled(600)); }
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::richtext` and `& $fc test --release --lib theme::rich_edit`. Expected: FAIL to compile with `cannot find type/function` errors naming `diff_edit`, `adjust`, `ranges`, `range_chip`, `RichEditModel`, `Toggle` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.** `diff_edit`: take the common prefix, then the common suffix that does not overlap it, then snap both back to char boundaries. `adjust`: cut out `start..start+removed`, shift spans to the right of it by `inserted − removed`, add `inherit` over `start..start+inserted`, then merge.
- [ ] **Step 4: Run** both modules, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): rich text editing — diff, adjust, ranges and the editor model (FEAT-1 A5)`.

### Task A6: Overlay types, logo policy, clock text and save validation

**Files:**
- Create: `src/theme/overlay/mod.rs`
- Modify: `src/theme/mod.rs` (`pub mod overlay;`)

**Interfaces:**
- Consumes: `StyleSpan`, `richtext::normalize` (A4); strings (A1).
- Produces (exact shapes from spec §4.5; all derive `Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash`):
  `SlotKey = String`; `saved_key(id) -> SlotKey` (`"saved:<uuid>"`); `saved_id(key: &str) -> Option<Uuid>`; `OverlaySlot { key, z: u32, inst: u64, name, source: OverlaySource, #[serde(default)] place: Placement }`; `OverlaySource { Text { text, #[serde(default)] styles, #[serde(default)] look }, Image { source: ImageSource, #[serde(default = "full")] opacity: u8 }, Clock { #[serde(default)] look, #[serde(default)] seconds, #[serde(default)] h24 }, #[serde(other)] Unknown }` (tag `type`, snake_case; every `look` field carries its own `#[serde(default)]`, see Global Constraints "Serde compatibility"); `ImageSource { ChurchLogo, Asset { asset } }` (tag `kind`); `OverlayLook` (`#[serde(default)]`, defaults `""`, 48, `"#ffffff"`, 600, true, `None`, `"#000000"`, 60); `BoxKind { #[default] None, Behind, Bar }`; `Placement { anchor: Anchor, width: u8, margin: u8 }` (`#[serde(default)]`, `Default` = `default_for(OverlayKind::Picture)`); `Anchor { TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom, BottomRight }`; `Overlay { id, name, source, #[serde(default)] place }`; `OVERLAY_FADE_MS: u32 = 400`; `MAX_SAVED_ON: usize = 8`.
  ```rust
  pub enum OverlayKind { Text, Picture, Clock, Other }
  impl OverlaySource { pub fn survives_logo(&self) -> bool; pub fn kind(&self) -> OverlayKind; pub fn is_bound(&self) -> bool; }
  impl Placement { pub fn default_for(kind: OverlayKind) -> Placement; }  // plan decision 6
  pub struct BoundCtx { pub now: chrono::DateTime<chrono::Local> }       // FEAT-2 adds `timers: Arc<TimersView>`
  impl BoundCtx { pub fn at(now: chrono::DateTime<chrono::Local>) -> Self; } // the only way FEAT-1 builds one; FEAT-2 fills `timers` with its empty default here
  pub fn bound_text(src: &OverlaySource, ctx: &BoundCtx) -> Option<String>;
  pub fn validate(o: Overlay, asset_exists: impl Fn(&AssetId) -> bool) -> Result<Overlay, String>;
  ```
  `validate` does the following. It trims the name; empty → `OVERLAY_NAME_REQUIRED`. It clamps `width` to 5–100, `margin` ≤ 20 and `opacity` ≤ 100, and applies plan decision 7 to `look`. Text: blank text → `OVERLAY_TEXT_REQUIRED`, and `styles` are normalized. Image: a missing `Asset` → `PICTURE_GONE`. `Unknown` → `OVERLAY_KIND`. A nil id gets `new_v4`.

- [ ] **Step 1: Write the failing tests and add `pub mod overlay;` to `src/theme/mod.rs` (the new `src/theme/overlay/mod.rs` holds only the test block), with no implementation yet (Global Constraints, TDD order):**

```rust
#[test] fn unknown_overlay_source_loads_as_unknown() {
    let s: OverlaySource = serde_json::from_str(r#"{"type":"hologram","x":1}"#).unwrap();
    assert_eq!(s, OverlaySource::Unknown); assert!(!s.survives_logo());
}
#[test] fn logo_policy_per_source() { /* Text, Image, Clock → true */ }
#[test] fn slot_round_trips_and_image_opacity_defaults_full() {
    let s: OverlaySource = serde_json::from_str(r#"{"type":"image","source":{"kind":"church_logo"}}"#).unwrap();
    assert_eq!(s, OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 });
}
#[test] fn saved_key_round_trips() { let id = Uuid::new_v4(); assert_eq!(saved_id(&saved_key(id)), Some(id)); assert_eq!(saved_id("message"), None); }
#[test] fn clock_text_12_and_24_hour_edges() {
    let at = |h, m, s| BoundCtx::at(Local.with_ymd_and_hms(2026, 1, 4, h, m, s).unwrap());
    let clock = |seconds, h24| OverlaySource::Clock { look: OverlayLook::default(), seconds, h24 };
    assert_eq!(bound_text(&clock(false, false), &at(0, 0, 0)).unwrap(), "12:00 AM");
    assert_eq!(bound_text(&clock(false, false), &at(12, 0, 0)).unwrap(), "12:00 PM");
    assert_eq!(bound_text(&clock(false, false), &at(9, 5, 0)).unwrap(), "9:05 AM");
    assert_eq!(bound_text(&clock(true, true), &at(23, 59, 59)).unwrap(), "23:59:59");
    assert_eq!(bound_text(&OverlaySource::Unknown, &at(1, 0, 0)), None);
}
#[test] fn clock_without_look_loads_with_defaults() {
    let s: OverlaySource = serde_json::from_str(r#"{"type":"clock"}"#).unwrap();
    assert_eq!(s, OverlaySource::Clock { look: OverlayLook::default(), seconds: false, h24: false });
    let t: OverlaySource = serde_json::from_str(r#"{"type":"text","text":"Hi"}"#).unwrap();
    assert!(matches!(t, OverlaySource::Text { ref look, ref styles, .. } if *look == OverlayLook::default() && styles.is_empty()));
}
#[test] fn overlay_without_place_loads_with_default_placement() {
    let o: Overlay = serde_json::from_value(json!({"id": Uuid::nil(), "name": "Logo", "source": {"type":"image","source":{"kind":"church_logo"}}})).unwrap();
    assert_eq!(o.place, Placement::default());
}
#[test] fn overlay_save_validates() {
    // blank name → OVERLAY_NAME_REQUIRED; text "  " → OVERLAY_TEXT_REQUIRED; Asset "gone.png" with asset_exists false → PICTURE_GONE;
    // Unknown → OVERLAY_KIND; width 200 → 100, width 0 → 5; nil id replaced
}
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::overlay`. Expected: FAIL to compile with `cannot find type/function` errors naming `OverlaySource`, `ImageSource`, `OverlayLook`, `BoundCtx`, `bound_text`, `saved_key`, `validate` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): overlay slot types, logo policy, clock text, save validation (FEAT-1 A6)`.

### Task A7: Overlay slot rules (show, hide, cap, clear epoch, failures, chips) and asset references

**Files:**
- Create: `src/theme/overlay/slots.rs`, `src/theme/assets.rs`
- Modify: `src/theme/overlay/mod.rs` (`pub mod slots;`), `src/theme/mod.rs` (`pub mod assets;`)

**Interfaces:**
- Consumes: A2, A3, A6.
- Produces in `overlay::slots` (pure functions over `Vec<OverlaySlot>`, which Part B's `Frame.overlays` and `Live` call; none of them bumps a version):
  ```rust
  pub fn show(slots: &mut Vec<OverlaySlot>, slot: OverlaySlot) -> bool;   // insert/replace by key, keep sorted by (z, key); false if an equal copy ignoring inst is there
  pub fn hide(slots: &mut Vec<OverlaySlot>, key: &str) -> bool;
  pub fn retain(slots: &mut Vec<OverlaySlot>, f: impl Fn(&OverlaySlot) -> bool) -> bool;
  pub fn saved_on(slots: &[OverlaySlot]) -> usize;                         // only "saved:" keys
  pub fn check_seen_clear(seen: Option<u64>, epoch: u64) -> Result<(), String>; // None or < epoch → SCREEN_CLEARED
  pub fn saved_slot(id: Uuid, library: &[Overlay], logo: Option<&AssetId>, inst: u64) -> Result<OverlaySlot, String>; // OVERLAY_GONE; ChurchLogo → Asset{logo} or LOGO_FIRST; z = library index
  pub fn turn_on(slots: &mut Vec<OverlaySlot>, slot: OverlaySlot) -> Result<bool, String>; // OVERLAY_CAP when 8 saved are on and key not among them; then show
  pub fn failed(slots: &mut Vec<OverlaySlot>, key: &str, inst: u64) -> Option<String>; // removes only when key AND inst match; returns the slot name for crate::theme::strings::overlay_failed
  #[derive(Clone, Debug, PartialEq)] pub struct Chip { pub id: Uuid, pub name: String, pub kind: OverlayKind, pub on: bool, pub changed: bool, pub deleted: bool }
  pub fn chips(library: &[Overlay], slots: &[OverlaySlot], logo: Option<&AssetId>) -> Vec<Chip>; // library order; then on-screen saved slots whose overlay is gone (deleted = true). changed = on && slot (name, source, place) != library copy with ChurchLogo resolved through `logo`, so a logo bug whose logo changed shows as changed
  #[derive(Serialize, Clone, Debug, PartialEq)] pub struct RemoteOverlay { pub id: Uuid, pub name: String, pub on: bool, pub kind: &'static str } // "text" | "picture" | "clock"
  pub fn remote_list(library: &[Overlay], slots: &[OverlaySlot]) -> Vec<RemoteOverlay>; // deleted → name "(deleted) <name>"; timer/message slots never listed
  ```
- Produces in `assets`:
  ```rust
  pub fn referenced(themes: &[SavedTheme], backgrounds: &[BackgroundEntry], overlays: &[Overlay]) -> BTreeSet<AssetId>; // theme backgrounds, entry asset + thumb, ImageSource::Asset
  pub fn slot_assets(slots: &[OverlaySlot]) -> BTreeSet<AssetId>;
  ```

- [ ] **Step 1: Write the failing tests and add `pub mod slots;` to `src/theme/overlay/mod.rs` and `pub mod assets;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order)** (pure versions of the spec's `live::tests` overlay rows):

```rust
#[test] fn overlay_on_bumps_and_is_idempotent() { /* show(s) → true; show(same with new inst) → false */ }
#[test] fn overlay_on_changed_copy_replaces() {}
#[test] fn overlay_off_when_absent_is_noop() {}
#[test] fn slots_stay_sorted_by_z_then_key() {}
#[test] fn ninth_saved_overlay_is_rejected() {
    // 8 saved on; turn_on(9th) → Err(OVERLAY_CAP); turn_on(one of the 8, edited) → Ok(true)
}
#[test] fn cap_ignores_non_saved_slots() { /* 7 saved + "message" + "timer:x" (Unknown source) → 8th saved Ok */ }
#[test] fn overlay_on_with_stale_seen_clear_is_refused() { assert_eq!(check_seen_clear(Some(3), 4).unwrap_err(), SCREEN_CLEARED); assert!(check_seen_clear(Some(4), 4).is_ok()); }
#[test] fn overlay_on_without_seen_clear_is_refused() { assert!(check_seen_clear(None, 0).is_err()); }
#[test] fn logo_retain_keeps_text_picture_clock_drops_unknown() { /* retain(|s| s.source.survives_logo()) */ }
#[test] fn logo_bug_follows_logo_change_on_next_toggle() {
    // saved_slot with logo "a.png" → Asset a; again with "b.png" → Asset b
}
#[test] fn logo_bug_without_logo_errors() { /* LOGO_FIRST */ }
#[test] fn gone_overlay_errors() { /* OVERLAY_GONE */ }
#[test] fn overlay_failed_removes_slot_and_names_it() {}
#[test] fn stale_overlay_failed_report_ignored() { /* inst mismatch → None, slot kept */ }
#[test] fn chips_mark_changed_and_deleted() {}
#[test] fn remote_view_lists_saved_overlays_with_state() { /* includes "(deleted) Wi-Fi" */ }
// assets
#[test] fn referenced_assets_include_themes_backgrounds_overlays() {}
#[test] fn referenced_assets_plus_live_frame_covers_deleted_overlay() { /* overlay deleted from library but slot on → slot_assets has it */ }
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::overlay::slots` and `& $fc test --release --lib theme::assets`. Expected: FAIL to compile with `cannot find type/function` errors naming `show`, `hide`, `turn_on`, `check_seen_clear`, `saved_slot`, `failed`, `chips`, `remote_list`, `referenced`, `slot_assets` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): overlay slot rules, clear epoch check, failure reports, chips, asset references (FEAT-1 A7)`.

### Task A8: Bundled bold and italic faces

**Files:**
- Create: `assets/fonts/NotoSans-Bold.ttf`, `assets/fonts/NotoSans-Italic.ttf`, `src/theme/paint/mod.rs`, `src/theme/paint/fonts.rs`
- Modify: `src/theme/mod.rs` (`pub mod paint;`)

**Interfaces:**
- Produces: `pub const RICH_FACES: [&[u8]; 2]` (`include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf")`, `…Italic.ttf`) and `pub fn add_rich_faces(fonts: &mut Fonts)` (`fonts.sys.db_mut().load_font_data(…)` for each). Part B Task B5 calls it from `Fonts::new` and `Fonts::bundled_only`.

- [ ] **Step 1: Get the fonts.** Use the same Noto Sans release as the existing `assets/fonts/NotoSans-Regular.ttf`: read its version from the `name` table (`fc-query` or Python `fontTools`), then take `NotoSans-Bold.ttf` and `NotoSans-Italic.ttf` (static TTF, hinted) from `https://github.com/notofonts/latin-greek-cyrillic/releases` at that version. Ask the user before downloading (state file names, source and size, about 0.6 MB each). `assets/fonts/OFL.txt` already covers them.
- [ ] **Step 2: Write the failing test and add the module lines.** Add `pub mod paint;` to `src/theme/mod.rs`; create `src/theme/paint/mod.rs` with `pub mod fonts;`; create `src/theme/paint/fonts.rs` holding only this test block:

```rust
#[test]
fn bold_and_italic_noto_faces_are_available_after_adding() {
    let mut f = Fonts::bundled_only();
    add_rich_faces(&mut f);
    let has = |w: u16, italic: bool| f.sys.db().faces().any(|x| x.families.iter().any(|(n, _)| n == "Noto Sans") && x.weight.0 == w && (x.style == fontdb::Style::Italic) == italic);
    assert!(has(700, false)); assert!(has(400, true)); assert!(has(600, false));
}
```
(Use `cosmic_text::fontdb` and `crate::render::Fonts`.)
- [ ] **Step 3: Run** `& $fc test --release --lib theme::paint::fonts`. Expected: FAIL to compile with `error[E0425]: cannot find function `add_rich_faces``.
- [ ] **Step 4: Implement** `RICH_FACES` and `add_rich_faces` above the test block, run the same command, then the whole library. Expected: PASS (1 test).
- [ ] **Step 5: Commit** `feat(theme): bundle Noto Sans Bold and Italic for rich text (FEAT-1 A8)`.

### Task A8b: Extra bundled font families

Spec decision 21 (default): Noto Serif Regular/Bold, Montserrat SemiBold/Bold and Oswald Medium are bundled too (about 2 MB, all OFL) and listed after Noto Sans as "works on every computer" in the theme editor's Font list (plan decision 10).

**Files:**
- Create: `assets/fonts/NotoSerif-Regular.ttf`, `assets/fonts/NotoSerif-Bold.ttf`, `assets/fonts/Montserrat-SemiBold.ttf`, `assets/fonts/Montserrat-Bold.ttf`, `assets/fonts/Oswald-Medium.ttf`, `assets/fonts/OFL-Montserrat.txt`, `assets/fonts/OFL-Oswald.txt`
- Modify: `src/theme/paint/fonts.rs` (this plan's own file)

**Interfaces:**
- Produces:
  ```rust
  /// Font list order for the theme editor (Task B8): bundled families first, each marked BUNDLED_FONT_NOTE.
  pub const BUNDLED_FAMILIES: [&str; 4] = ["Noto Sans", "Noto Serif", "Montserrat", "Oswald"];
  pub const EXTRA_FACES: [&[u8]; 5]; // include_bytes! of the five files above
  pub fn add_extra_faces(fonts: &mut Fonts); // load_font_data for each
  pub fn is_bundled(family: &str) -> bool;  // case-insensitive match against BUNDLED_FAMILIES
  ```
  Part B Task B5a calls `add_extra_faces` from `Fonts::new` and `Fonts::bundled_only` (after `add_rich_faces`); B8 builds the Font list from `BUNDLED_FAMILIES`.

- [ ] **Step 1: Get the fonts (ask first).** Ask the user before downloading, stating file names, sources and sizes: Noto Serif Regular and Bold (static hinted TTF, about 0.4 MB each) from `https://github.com/notofonts/latin-greek-cyrillic/releases` at the same release used in Task A8; Montserrat SemiBold and Bold (static TTF, about 0.2 MB each) from `https://github.com/JulietaUla/Montserrat` (`fonts/ttf/`); Oswald Medium (static TTF, about 0.1 MB) from `https://github.com/googlefonts/OswaldFont` (`fonts/ttf/`). Copy each project's `OFL.txt` to `assets/fonts/OFL-Montserrat.txt` and `assets/fonts/OFL-Oswald.txt` (Noto Serif is covered by the existing `assets/fonts/OFL.txt`). Put each download in its own empty directory first and check it is a TrueType file (`file` or the first 4 bytes `00 01 00 00`) before copying it into `assets/fonts/`.
- [ ] **Step 2: Write the failing tests** in the existing test block of `src/theme/paint/fonts.rs`:

```rust
#[test]
fn extra_families_are_available_after_adding() {
    let mut f = Fonts::bundled_only();
    add_extra_faces(&mut f);
    let has = |fam: &str, w: u16| f.sys.db().faces().any(|x| x.families.iter().any(|(n, _)| n == fam) && x.weight.0 == w);
    assert!(has("Noto Serif", 400)); assert!(has("Noto Serif", 700));
    assert!(has("Montserrat", 600)); assert!(has("Montserrat", 700));
    assert!(has("Oswald", 500));
}
#[test]
fn bundled_families_list_starts_with_noto_sans() {
    assert_eq!(BUNDLED_FAMILIES[0], crate::render::layout::BUNDLED_FAMILY);
    assert!(is_bundled("montserrat") && !is_bundled("Lato"));
}
```
- [ ] **Step 3: Run** `& $fc test --release --lib theme::paint::fonts`. Expected: FAIL to compile with `cannot find function `add_extra_faces`` and `cannot find value `BUNDLED_FAMILIES``.
- [ ] **Step 4: Implement**, run the same command (Expected: PASS, 3 tests), then the whole library (Expected: PASS; the existing goldens are untouched because `Fonts::bundled_only` does not call `add_extra_faces` until Part B, and the extra families never match "Noto Sans").
- [ ] **Step 5: Commit** `feat(theme): bundle Noto Serif, Montserrat and Oswald as extra families (FEAT-1 A8b)`.

### Task A9: Merge-by-key for `settings/save` and `service/update`

**Files:**
- Create: `src/theme/merge.rs`
- Modify: `src/theme/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn merge_object(stored: &Value, patch: &Value) -> Result<Value, String>;   // patch must be an object (else strings::EXPECTED_OBJECT); missing key keeps; null or a value replaces
  pub fn merge_service(stored: &Value, patch: &Value) -> Result<Value, String>;  // merge_object on the service; "items" (when present) is the patch's list: each item with an id found in stored is merge_object(stored_item, patch_item) unless its "kind" differs (then the patch item as is); unknown ids are taken as sent; duplicate ids in the patch → strings::ITEM_TWICE; a non-object item → strings::EXPECTED_OBJECT
  pub fn merge_typed<T: Serialize + DeserializeOwned>(stored: &T, patch: &Value, f: fn(&Value, &Value) -> Result<Value, String>) -> Result<T, String>;
  ```
  An explicit `null` clears an `Option` field. On a non-optional field the typed deserialize fails, and its message is returned.

- [ ] **Step 1: Write the failing tests and add `pub mod merge;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order)** (pure halves of spec §9.1 `app::tests` merge rows; uses `model::Settings`/`model::Service` as they are today plus raw JSON keys for fields that don't exist yet):

```rust
#[test] fn settings_save_missing_key_keeps_value() {
    let s = Settings { logo: Some("a.png".into()), ..Settings::default() };
    let out: Settings = merge_typed(&s, &json!({"lines_per_slide": 4}), merge_object).unwrap();
    assert_eq!((out.logo.as_deref(), out.lines_per_slide), (Some("a.png"), 4));
}
#[test] fn settings_save_null_clears_optional_field() { /* {"logo": null} → None */ }
#[test] fn settings_save_keeps_logo_set_meanwhile() { /* stored logo b; dialog patch without "logo" → b */ }
#[test] fn service_update_missing_keys_keep_every_new_field() {
    let stored = json!({"id": ID, "name": "Sun", "theme": T, "items": [{"id": I, "kind": "text", "title": "A", "body": "x", "styles": [{"start":0,"end":1,"bold":true}], "theme": T, "starts_at": "10:00", "advance": {"after_s": 5}, "stage_layout": L, "slide_notes": {}, "arrangement": [], "second": "KJV", "layout": "x", "breaks": [], "pco_item_id": "9"}]});
    let patch = json!({"id": ID, "name": "Sunday", "items": [{"id": I, "kind": "text", "title": "B", "body": "x"}]});
    let out = merge_service(&stored, &patch).unwrap();
    for k in ["styles","theme","starts_at","advance","stage_layout","slide_notes","arrangement","second","layout","breaks","pco_item_id"] { assert_eq!(out["items"][0][k], stored["items"][0][k], "{k}"); }
    assert_eq!((out["name"].as_str(), out["theme"].clone(), out["items"][0]["title"].as_str()), (Some("Sunday"), stored["theme"].clone(), Some("B")));
}
#[test] fn service_update_kind_change_replaces_item() {}
#[test] fn service_update_order_and_removal_follow_patch() {}
#[test] fn service_update_rejects_duplicate_ids_and_non_objects() {
    let stored = json!({"id": ID, "items": []});
    assert_eq!(merge_service(&stored, &json!({"items": [{"id": I, "kind": "text"}, {"id": I, "kind": "text"}]})).unwrap_err(), ITEM_TWICE);
    assert_eq!(merge_service(&stored, &json!([1, 2])).unwrap_err(), EXPECTED_OBJECT);
    assert_eq!(merge_object(&json!({}), &json!("x")).unwrap_err(), EXPECTED_OBJECT);
}
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::merge`. Expected: FAIL to compile with `cannot find type/function` errors naming `merge_object`, `merge_service`, `merge_typed` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): merge-by-key for settings and service updates (FEAT-1 A9)`.

### Task A10: Format 1 → 2 migration and labelled backup names

**Files:**
- Create: `src/theme/migrate.rs`
- Modify: `src/theme/mod.rs`

**Interfaces:**
- Produces (pure, on `serde_json::Value` and names; `store.rs` calls them in Part B):
  ```rust
  // No format constant here: store::SUPPORTED_FORMAT (Part B Task B2) is the only one (spec §4.7).
  pub fn read_format(bytes: &[u8]) -> Result<u32, String>; // deserializes only `struct Header { #[serde(default)] format: u32 }`, ignoring every other key; missing or 0 → 1; a JSON syntax error → Err (plan decision 15)
  #[derive(Debug, PartialEq)] pub enum LoadPlan { Current, Migrate { from: u32 }, Newer { found: u32 } }
  pub fn plan(found: u32, supported: u32) -> LoadPlan;
  pub fn migrate_1_to_2(data: &mut Value, now: DateTime<Utc>); // adds "backgrounds" entries, sets "format" to the literal 2 (this step's own target)
  pub fn labeled_backup_name(stamp: &str, label: &str) -> String; // "backup-<stamp>-<label>", label kept to [a-z0-9-]
  pub fn backup_label(name: &str) -> Option<&str>;      // stamp is the 17-char now_stamp(): "2026-11-01_101530"
  pub fn backups_to_prune(names: &[String], keep: usize) -> Vec<String>; // newest `keep` unlabelled kept + newest per label kept; rest returned
  pub fn before_update_copy_name(stamp: &str) -> String; // "data.before-update-<stamp>.json"
  ```
  `migrate_1_to_2` visits `settings.background` first, then every `services[].items[].background` in order. For each distinct image/video `(type, asset)` it adds `{id, name, background, status: {"state":"ready"}, added}`. The name is the `media[].name` whose `asset` matches, otherwise `strings::numbered_background(n)` ("Background 1", "Background 2", …, numbered across the unnamed ones). Values already present in `backgrounds` are skipped, so a second run adds nothing. No other key changes.

  **How Part B uses the result (plan decision 15).** `read_format` → `Err` (JSON syntax error): the existing "could not be read" path, unchanged. `Ok(n)` with `plan(n, SUPPORTED_FORMAT) == Newer`: try the full typed parse; on failure start **read-only with empty data** (not "could not be read").

- [ ] **Step 1: Write the failing tests and add `pub mod migrate;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order):**

```rust
const V02: &str = include_str!("../../tests/fixtures/v02-data.json"); // Create this fixture in this task: format 1, settings image bg "a.jpg", two items with "a.jpg" and video "v.mp4", media [{asset:"v.mp4", name:"Waves"}]
#[test] fn read_format_defaults_to_1() { assert_eq!(read_format(b"{}").unwrap(), 1); assert_eq!(read_format(br#"{"format":0}"#).unwrap(), 1); }
#[test] fn read_format_syntax_error_is_err() { assert!(read_format(br#"{"format":3,"x":[}"#).is_err()); }
#[test] fn read_format_ignores_shapes_the_full_parse_would_reject() {
    // newer data: valid JSON whose fields this build can't parse still yields its format (→ read-only, not "could not be read")
    assert_eq!(read_format(br#"{"format":3,"services":"not a list","items":{"kind":"blank"}}"#).unwrap(), 3);
}
#[test] fn plan_classifies() { assert_eq!(plan(1, 2), LoadPlan::Migrate { from: 1 }); assert_eq!(plan(3, 2), LoadPlan::Newer { found: 3 }); assert_eq!(plan(2, 2), LoadPlan::Current); }
#[test] fn migrate_adds_library_entries_for_used_backgrounds() { /* 2 entries, format 2 */ }
#[test] fn migrate_names_entries_from_media() { /* "Background 1" for a.jpg, "Waves" for v.mp4 */ }
#[test] fn migrate_twice_adds_nothing() {}
#[test] fn migrate_sets_format_2_and_changes_no_other_key() { /* keys of V02 except "backgrounds"/"format" are equal before and after; v["format"] == 2 */ }
#[test] fn migrated_data_parses_as_backgrounds() { /* every entry deserializes as BackgroundEntry; added is RFC 3339 */ }
#[test] fn labeled_backup_survives_prune() {
    let names: Vec<String> = (0..12).map(|i| format!("backup-2026-11-{:02}_101530", i + 1)).chain(["backup-2026-10-01_090000-before-update".into()]).collect();
    let gone = backups_to_prune(&names, 10);
    assert_eq!(gone.len(), 2); assert!(!gone.iter().any(|n| n.ends_with("before-update")));
}
#[test] fn only_newest_labeled_backup_per_label_is_kept() {}
```
The fixture file `tests/fixtures/v02-data.json` is new (nothing else uses `tests/fixtures/`).
- [ ] **Step 2: Run** `& $fc test --release --lib theme::migrate`. Expected: FAIL to compile with `cannot find type/function` errors naming `read_format`, `plan`, `LoadPlan`, `migrate_1_to_2`, `backups_to_prune` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): format 1→2 migration and labelled backup names (FEAT-1 A10)`.

### Task A11: Rich runs — styles to shaped, coloured, underlined text

**Files:**
- Create: `src/theme/paint/runs.rs`
- Modify: `src/theme/paint/mod.rs`

**Interfaces:**
- Consumes: `StyleSpan` (A4), `add_rich_faces` (A8), `render::{Fonts, parse_color}`.
- Produces (the spec's `layout::shape_runs`; Part B Task B5 makes `layout.rs` call it):
  ```rust
  pub struct RunBase { pub weight: u16, pub color: [u8; 4] }
  #[derive(Clone, Debug, PartialEq)]
  pub struct RunSpec { pub range: Range<usize>, pub weight: u16, pub italic: bool, pub underline: bool, pub color: [u8; 4], pub scale: f32 }
  pub fn run_specs(text: &str, spans: &[StyleSpan], base: &RunBase) -> Vec<RunSpec>; // covers 0..len contiguously; bold → 700 (never above); size% → scale (0 = 1.0); colour via parse_color
  pub fn shape_runs(fonts: &mut Fonts, text: &str, specs: &[RunSpec], family: &str, px: f32, line: f32, width: f32, align: cosmic_text::Align) -> cosmic_text::Buffer; // set_rich_text, one Attrs per spec with .metadata(index) and .metrics(Metrics::new(px*scale, px*scale*line)), Wrap::WordOrGlyph, shaped
  pub fn reshape(fonts: &mut Fonts, buf: &mut Buffer, text: &str, specs: &[RunSpec], family: &str, px: f32, line: f32, align: Align); // fit steps
  pub fn paint_glyphs(fonts: &mut Fonts, buf: &mut Buffer, pix: &mut tiny_skia::Pixmap, at: (f32, f32), opacity: f32); // per-glyph colour from attrs
  #[derive(Clone, Debug, PartialEq)] pub struct UnderlineRect { pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub color: [u8; 4] }
  pub fn underline_rects(buf: &Buffer, specs: &[RunSpec]) -> Vec<UnderlineRect>; // per line, adjacent glyphs of one underlined spec merged; y = line_y + 0.06*font_size, h = 0.05*font_size (cosmic-text decoration metrics if 0.19 exposes them)
  ```

- [ ] **Step 1: Write the failing tests and add `pub mod runs;` to `src/theme/paint/mod.rs`, with no implementation yet (Global Constraints, TDD order):**

```rust
#[test] fn bold_run_in_semibold_theme_is_700() {
    let s = run_specs("Hello world", &[span(6, 11, bold)], &RunBase { weight: 600, color: WHITE });
    assert_eq!(s.iter().map(|r| (r.range.clone(), r.weight)).collect::<Vec<_>>(), vec![(0..6, 600), (6..11, 700)]);
}
#[test] fn bold_run_in_bold_theme_stays_700() {}
#[test] fn colored_run_and_huge_size() { /* color #f5b94a → [245,185,74,255]; size 150 → scale 1.5 */ }
#[test] fn no_spans_is_one_run_with_theme_values() {}
#[test] fn shaped_bold_run_uses_the_700_face() {
    // Fonts::bundled_only + add_rich_faces; shape "a b" with b bold; the glyph with metadata 1 has a font id whose face weight is 700
}
#[test] fn underline_rect_sits_under_its_word_only() {}
#[test] fn painted_colored_run_has_its_color() { /* paint into 400x100 pixmap; some pixel ≈ gold where the gold run is, none in the white run's box */ }
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::paint::runs`. Expected: FAIL to compile with `cannot find type/function` errors naming `RunBase`, `RunSpec`, `run_specs`, `shape_runs`, `paint_glyphs`, `underline_rects` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.** `paint_glyphs` uses `Buffer::draw` with the default colour from the first spec. cosmic-text passes each glyph's `color_opt`, which comes from the `Attrs` colour.
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): rich run shaping, per-run colour and underline (FEAT-1 A11)`.

### Task A12: Outline and text band helpers

**Files:**
- Create: `src/theme/paint/decor.rs`
- Modify: `src/theme/paint/mod.rs`

**Interfaces:**
- Produces (Part B `raster.rs` applies them to its private `Coverage`):
  ```rust
  pub fn outline_px(outline: u8, scale: f32) -> usize;                              // round(outline * scale)
  pub fn dilate(data: &[f32], w: usize, h: usize, r: usize) -> Vec<f32>;            // separable max filter (rows then columns), square kernel 2r+1
  pub fn band_box(text: render::Rect, scale: f32, canvas: (u32, u32)) -> (render::Rect, f32); // grown by 28*scale each side, clipped to canvas; radius 12*scale
  pub fn fill_round_rect(pix: &mut Pixmap, r: render::Rect, radius: f32, color: [u8; 4], opacity: f32);
  ```

- [ ] **Step 1: Write the failing tests and add `pub mod decor;` to `src/theme/paint/mod.rs`, with no implementation yet (Global Constraints, TDD order):** `dilate_single_pixel_makes_square` (5×5 grid, centre 1.0, r=1 → the 3×3 block is 1.0, the rest 0), `dilate_r0_is_identity`, `dilate_clips_at_edges`, `outline_thick_is_6_logical_px` (`outline_px(6, 0.5) == 3`), `band_box_pads_28_and_clips` (text box at x=10 on a 1920 canvas → x=0 after clipping), and `round_rect_fills_center_not_corner`.
- [ ] **Step 2: Run** `& $fc test --release --lib theme::paint::decor`. Expected: FAIL to compile with `cannot find type/function` errors naming `outline_px`, `dilate`, `band_box`, `fill_round_rect` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement** (`tiny_skia::PathBuilder` for the rounded rectangle).
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): outline dilation and text band helpers (FEAT-1 A12)`.

### Task A13: Background painter (colour, gradient, picture, Darken)

**Files:**
- Create: `src/theme/paint/background.rs`
- Modify: `src/theme/paint/mod.rs`

**Interfaces:**
- Consumes: `Background` (A2), `crate::render::{Rendered, parse_color}`, `crate::render::layout::clamp_canvas`.
- Produces (the spec's `render::background::background_pixels`, which FEAT-7 calls `background_picture`):
  ```rust
  pub fn background_pixels(bg: &Background, decoded: Option<&image::RgbaImage>, canvas: (u32, u32)) -> Rendered; // opaque; font_px 0, font_fallback false
  pub fn gradient_line(angle: u16, w: f32, h: f32) -> ((f32, f32), (f32, f32)); // CSS geometry, plan decision 8
  pub fn cover_crop(img: (u32, u32), canvas: (u32, u32)) -> (u32, u32, u32, u32); // centred crop with the canvas aspect
  ```
  Colour → fill. Gradient → `LinearGradient` along `gradient_line`. Image or Video with `decoded` → cover-fit and scale (`image::imageops::resize`, `Triangle`). Image or Video without `decoded` → opaque black. Then a black layer at `dim/100` (the dim is clamped to 80).

- [ ] **Step 1: Write the failing tests and add `pub mod background;` to `src/theme/paint/mod.rs`, with no implementation yet (Global Constraints, TDD order):**

```rust
#[test] fn gradient_background_180_runs_top_to_bottom() { /* Midnight at 192x108: top row ≈ #0b1a33, bottom ≈ #000000 (±6) */ }
#[test] fn gradient_background_135_runs_corner_to_corner() { /* top-left ≈ from, bottom-right ≈ to */ }
#[test] fn gradient_line_matches_css_for_90() {}
#[test] fn dim_50_image_background() { /* 4x4 white image → every pixel ≈ 128 */ }
#[test] fn dim_is_capped_at_80() {}
#[test] fn cover_crop_centres() { assert_eq!(cover_crop((400, 100), (200, 100)), (100, 0, 200, 100)); }
#[test] fn missing_picture_is_black_not_panic() {}
#[test] fn zero_canvas_is_clamped() { /* canvas (0,0) → 1x1 */ }
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::paint::background`. Expected: FAIL to compile with `cannot find type/function` errors naming `background_pixels`, `gradient_line`, `cover_crop` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): background painter with gradients and darken (FEAT-1 A13)`.

### Task A14: Overlay drawing and its cache key

**Files:**
- Create: `src/theme/paint/overlay.rs`
- Modify: `src/theme/paint/mod.rs`

**Interfaces:**
- Consumes: A6 types, `bound_text` result passed in as `text`, A11 (`run_specs`, `shape_runs`, `reshape`, `paint_glyphs`, `underline_rects`), A12 (`fill_round_rect`), `crate::render::{Fonts, Rendered}`, `crate::render::layout::{BUNDLED_FAMILY, CANVAS_W, clamp_canvas}`. It does **not** use `render::fit_steps` (26 px floor, raises sizes below 26); it has its own `overlay_fit_steps` (plan decision 11).
- Produces (the spec's `render::overlay::draw`):
  ```rust
  pub struct Placed { pub x: u32, pub y: u32, pub img: Rendered }   // img holds only the bounding box
  #[derive(Clone, Debug, PartialEq, Eq, Hash)]
  pub struct OverlayKey { pub source: OverlaySource, pub text: Option<String>, pub place: Placement, pub theme_font: String, pub canvas: (u32, u32) }
  impl OverlayKey { pub fn of(slot: &OverlaySlot, text: Option<&str>, theme_font: &str, canvas: (u32, u32)) -> Self; } // never includes key, inst or name
  pub fn anchor_pos(anchor: Anchor, margin: u8, canvas: (u32, u32), size: (u32, u32)) -> (u32, u32); // margin*10 logical px scaled by canvas.w/1920; result clamped so the box stays inside
  pub fn draw(fonts: &mut Fonts, slot: &OverlaySlot, text: Option<&str>, theme_font: &str, canvas: (u32, u32), picture: Option<&image::RgbaImage>) -> Placed;
  pub const MIN_OVERLAY_PX: f32 = 18.0; pub const BOX_PAD: f32 = 24.0; pub const MAX_LINES: usize = 3; pub const BEHIND_RADIUS: f32 = 10.0;
  /// Sizes to try, largest first: −4 px above 60 px, else −2 px, ending at MIN_OVERLAY_PX.
  /// Starts at `max` itself (a `max` below the floor is used as is, never raised).
  pub fn overlay_fit_steps(max: f32) -> impl Iterator<Item = f32>;
  ```
  The rules come from spec §6.4 and plan decision 6:
  - The box width is `place.width` % of the canvas width. The text wrap width is the box width minus `2*BOX_PAD*s` when boxed.
  - Text (`Text` with its `styles`, or a bound `Clock` string with no styles) starts at `look.size × s` (s = canvas.w / 1920) and shrinks through `overlay_fit_steps(look.size × s)` until it fits in 3 lines; at the 18 px floor it is drawn at 18 even if longer (clipped to 3 lines). `look.font == ""` uses `theme_font`, and a missing family uses Noto Sans. The weight is `look.weight`. Alignment follows the anchor column (Left/Center/Right).
  - `Behind` draws a rounded box behind the lines. `Bar` is full width, flush on the anchor's edge row (Top row → y 0, Bottom row → bottom, middle row → vertical centre).
  - Pictures scale to the box width keeping their aspect ratio (height capped at the canvas height) and are drawn at `opacity`.
  - `Unknown`, an `Image` without `picture`, or a `Clock` with `text` `None` → a 0×0 `Placed`.
  - Text shadow is **not** drawn in Part A (raster's shadow is private). Part B Task B5 adds it.

- [ ] **Step 1: Write the failing tests and add `pub mod overlay;` to `src/theme/paint/mod.rs`, with no implementation yet (Global Constraints, TDD order):**

```rust
#[test] fn overlay_text_bar_bottom() { /* 1920x1080, Text "Welcome!" Bar Bottom → x 0, w 1920, y + h == 1080; bar pixels ≈ #000000 at 60% */ }
#[test] fn overlay_picture_top_right_scaled() { /* 200x100 picture, width 12, margin 4 → w 230, h 115, x = 1920-230-40, y = 40 */ }
#[test] fn overlay_on_4x3_keeps_corner() { /* 1024x768, BottomRight → x + w == 1024 - round(40*1024/1920) */ }
#[test] fn overlay_clock_box() { /* Clock with text "9:05 AM", Behind → non-empty, lies inside width*canvas */ }
#[test] fn overlay_draw_is_bounding_box_only() { /* img.width < canvas width for a 25% picture */ }
#[test] fn overlay_fit_steps_stop_at_18_and_never_raise() {
    assert_eq!(overlay_fit_steps(64.0).collect::<Vec<_>>()[..3], [64.0, 60.0, 58.0]);
    assert_eq!(overlay_fit_steps(24.0).collect::<Vec<_>>(), vec![24.0, 22.0, 20.0, 18.0]);
    assert_eq!(overlay_fit_steps(20.0).next(), Some(20.0)); // not raised to 26 like render::fit_steps
}
#[test] fn overlay_size_20_draws_at_20() { /* Text "Hi", look.size 20, 1920 canvas → the drawn font size (Placed.img.font_px) == 20 */ }
#[test] fn long_text_shrinks_to_three_lines_min_18() { /* 400 words, width 20 % at 1920 → font_px == 18, at most 3 lines */ }
#[test] fn overlay_never_leaves_canvas() {
    // for canvas in [(0,0),(1024,768),(1920,1080)], picture 1x4000 and 4000x1, width 100, margin 20, every anchor:
    // x + w <= canvas.w && y + h <= canvas.h (after clamp_canvas)
}
#[test] fn unknown_and_missing_picture_draw_nothing() {}
#[test] fn overlay_key_ignores_inst_key_and_name_but_not_bound_text() {}
```

- [ ] **Step 2: Run** `& $fc test --release --lib theme::paint::overlay`. Expected: FAIL to compile with `cannot find type/function` errors naming `draw`, `anchor_pos`, `OverlayKey`, `overlay_fit_steps`, `MIN_OVERLAY_PX` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): overlay drawing and overlay cache key (FEAT-1 A14)`.

### Task A15: Background picker model

**Files:**
- Create: `src/theme/picker.rs`
- Modify: `src/theme/mod.rs`

**Interfaces:**
- Consumes: A2, `model::{MediaFile, MediaKind, MediaStatus}`, strings.
- Produces (the pure core of Plan 3's `BackgroundPicker`, which Part B Task B9 calls from `src/ui/dialogs/background.rs`):
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PickerMode { ItemEdit, SavedTheme, StandardTheme, Manager }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Section { Current, Inherit, Colors, Library, Media }
  #[derive(Clone, Debug, PartialEq)]
  pub struct PickerChoice { pub section: Section, pub label: String, pub value: Option<Background>, pub pickable: bool, pub status: Option<String>, pub video: bool, pub entry: Option<Uuid> }
  pub fn choices(entries: &[BackgroundEntry], media: &[MediaFile], mode: PickerMode, current: Option<&Background>) -> Vec<PickerChoice>;
  pub fn selected(choices: &[PickerChoice], current: Option<&Background>) -> Option<usize>; // same_source match; None current → Inherit row
  pub fn darken_shown(value: Option<&Background>) -> bool;                                  // Image | Video | Gradient
  ```
  The section order follows spec §3.4:
  - **Current** appears only when `current` is `Some` and matches no other choice.
  - **Inherit** is "From theme" in ItemEdit and "Standard background" in SavedTheme; StandardTheme and Manager have no Inherit row.
  - **Colors** lists the six colours, then the six gradients, from `builtin_backgrounds`.
  - **Library** has a status of `converting(p)` / `COULDNT_CONVERT` and `pickable = entry.pickable()`.
  - **Media** lists ready image and video `MediaFile`s whose asset is not already in Library.
  - Manager mode lists Library only.

- [ ] **Step 1: Write the failing tests and add `pub mod picker;` to `src/theme/mod.rs`, with no implementation yet (Global Constraints, TDD order):** `background_picker_sections_in_order`, `picker_keeps_removed_library_background` (current Image "gone.jpg" → first row is Current, selected index 0), `converting_background_not_pickable` (status "Converting 40 %", pickable false), `failed_entry_says_couldnt_convert`, `media_already_in_library_is_not_repeated`, `selected_ignores_dim` (current Gradient Midnight dim 30 → selects Midnight), `darken_only_for_picture_video_gradient`, `manager_lists_library_only`.
- [ ] **Step 2: Run** `& $fc test --release --lib theme::picker`. Expected: FAIL to compile with `cannot find type/function` errors naming `PickerMode`, `Section`, `choices`, `selected`, `darken_shown` (0 tests run means the `pub mod` line is missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the module, then the whole library. Expected: PASS.
- [ ] **Step 5: Commit** `feat(theme): background picker model with sections (FEAT-1 A15)`.

---

# PART B — Integration after Plan 3 and Plan 4 merge

> **BLOCKED** until `native-foundation` contains Plan 3 (operator) **and** Plan 4 (imports, recovery, packaging, Tauri removal), and Part A is merged on top. Before starting each task, read the files it names as they are then. Plan 3 and Plan 4 describe behaviour, not line numbers. After Plan 4 Task 12 there is one binary.
>
> **Commands used in Part B** (run from the repo root after the merge):
> - unit tests in one module: `cargo test --locked --lib <module path>` (e.g. `cargo test --locked --lib live::`)
> - goldens: `cargo test --locked --test render_golden [<name>]`
> - UI tests: `cargo test --locked --test ui_operator_flows [<name>]` (or the per-dialog test file Plan 3 created for that dialog; use the same file Plan 3 Task 18/19/20 used)
> - full check at the end of every task: `cargo test --locked` (debug), Expected: PASS with no existing test changed unless the task says so.
>
> Spec test names are in spec §9; each task lists every test it owns by name. Tasks with a letter suffix (B4a, B4b, …) are split for size; run them in order.

### Task B1: Move files to spec paths and wire the types into the data model

**Files:**
- Move (`git mv`, no content change): `src/theme/overlay/mod.rs` → `src/overlay.rs`; `src/theme/overlay/slots.rs` → `src/overlay/slots.rs`; `src/theme/richtext.rs` → `src/richtext.rs`; `src/theme/backgrounds.rs` → `src/backgrounds.rs`; `src/theme/paint/overlay.rs` → `src/render/overlay.rs`; `src/theme/paint/background.rs` → `src/render/background.rs`.
- Modify: `src/lib.rs`, `src/theme/mod.rs`, `src/theme/paint/mod.rs`, `src/render/mod.rs`, `src/model.rs`, `src/theme/saved.rs`, `src/strings/mod.rs` (create only if missing).

**Interfaces:**
- `src/lib.rs`: `pub mod overlay; pub mod richtext; pub mod backgrounds;` and `pub mod strings;` only if missing.
- `src/strings/mod.rs`: add `pub mod themes;` (create the file with that line if no other plan has).
- `src/theme/mod.rs`: remove `pub mod overlay; pub mod richtext; pub mod backgrounds;` and the `#[path] pub mod strings;` line; add `pub use crate::{overlay, richtext, backgrounds}; pub use crate::strings::themes as strings;`.
- `src/theme/paint/mod.rs`: remove `pub mod overlay; pub mod background;`; add `pub use crate::render::{overlay, background};`.
- `src/render/mod.rs`: `pub mod overlay; pub mod background;`.
- `model.rs`: delete the old `Theme` and `Background` definitions and add `pub use crate::theme::{Theme, TextAlign}; pub use crate::backgrounds::Background;` so `model::Theme`/`model::Background` keep their names.
- `Data` gains `themes: Vec<SavedTheme>`, `default_theme: Option<ThemeId>`, `backgrounds: Vec<BackgroundEntry>`, `overlays: Vec<Overlay>` (all `#[serde(default)]`; the Vecs `skip_serializing_if = "Vec::is_empty"`, `default_theme` `skip_serializing_if = "Option::is_none"`).
- `Service.theme` and `ServiceItem.theme` are `Option<ThemeId>` (`default, skip_serializing_if none`). `ItemKind::Text` gains `styles: Vec<StyleSpan>` (`default, skip_serializing_if empty`).
- `Data::catalog(&self) -> theme::Catalog<'_>` (standard style `settings.theme`, standard background `settings.background`).
- Free functions in `src/theme/saved.rs` (spec §4.2; FEAT-6 and FEAT-7 call these):
  ```rust
  pub fn find(data: &Data, id: ThemeId) -> Option<SavedTheme>;          // data.catalog().find(id)
  pub fn church_default(data: &Data) -> SavedTheme;                     // data.catalog().church_default()
  pub fn resolve(data: &Data, service: Option<&Service>, item: &ServiceItem) -> Resolved; // draws_text true for Song, Scripture, Text
  pub fn service_of<'a>(data: &'a Data, item_id: Uuid) -> Option<&'a Service>;
  ```
- `Data::referenced_assets` adds `theme::assets::referenced(..)` and handles `Background::Gradient` (no asset).
- `Data::after_load(&mut self) -> bool`: FEAT-1 adds `richtext::normalize` of every `ItemKind::Text` and `Overlay` text span list, returning true when anything changed. Later FEAT specs add their fix-ups here in the order given in spec §4.7.

- [ ] **Step 1: Move the files** with the six `git mv` commands and make the module-line changes above (no logic changes). Run `cargo build --locked`, then `cargo test --locked --lib`. Expected: builds, and the same Part A tests pass as before; the moved files' tests now run under `overlay::`, `richtext::`, `backgrounds::`, `render::overlay::` and `render::background::` instead of `theme::…`.
- [ ] **Step 2: Write the failing tests:** in `model`: `old_data_json_loads_with_defaults` (the v0.2 fixture from A10), `referenced_assets_include_themes_backgrounds_overlays` (Data level), `after_load_normalizes_stored_spans`; in `theme::saved`: `theme_find_reads_saved_and_builtin_themes`, `church_default_reads_data_default_theme`, `church_default_dangling_falls_back_to_standard`, `resolve_wrapper_uses_item_then_service_then_default`, `service_of_finds_item_in_unselected_service`; and a path test in `lib` `spec_paths_resolve` that only `use`s `crate::overlay::OverlaySource`, `crate::richtext::StyleSpan`, `crate::backgrounds::Background`, `crate::render::overlay::draw`, `crate::render::background::background_pixels`, `crate::strings::themes::THEME_SAVED`, `crate::theme::{find, church_default, resolve, service_of}`.
- [ ] **Step 3: Run** `cargo test --locked --lib model::` and `cargo test --locked --lib theme::saved`. Expected: FAIL to compile (`no field `themes` on type `Data``, `cannot find function `find` in module `theme``).
- [ ] **Step 4: Implement.** Fix call sites that build `Background::Image { asset }` / `Video { asset }` (add `dim: 0`) in `app.rs`, `slides.rs`, `live.rs` and the UI glue. Run the two commands again. Expected: PASS.
- [ ] **Step 5: Run** `cargo test --locked`. Expected: PASS, and every golden unchanged.
- [ ] **Step 6: Commit** `feat(model): move FEAT-1 files to spec paths; themes, backgrounds, overlays and rich text in the data model (FEAT-1 B1)`.

### Task B2: Store — format constant, migration chain, read-only guard, labelled backups

**Files:** Modify `src/store.rs` (as Plan 4 left it), `src/app.rs` (read-only flag), Plan 4's recovery writer (`live-state.json`).

**Interfaces:**
- `pub const SUPPORTED_FORMAT: u32 = 2;` (the only format constant in the code base).
- `store::load` calls `migrate::read_format` and branches (plan decision 15):
  - `Err` (JSON syntax error) → the existing "could not be read" path, unchanged.
  - `plan(found, SUPPORTED_FORMAT) == Migrate` → `backup_labeled(&data, "before-update")`. On error, copy to `before_update_copy_name(now_stamp())` and log the missing assets. Then `migrate_1_to_2`, then save at once.
  - `Newer` → `Loaded { data, read_only: true }`. A full typed parse is tried; if it fails, `data` is `Data::default()` (never "could not be read").
  - Then `after_load()`, with a save when it returns true and the store is not read-only.
- `Store::backup_labeled(&self, data, label) -> io::Result<String>`. `prune_backups` uses `migrate::backups_to_prune`.
- `App` keeps `read_only: bool`. Every editing `do_action` returns `strings::themes::NEWER_DATA`, and `store.save` refuses. **Live control keeps working** (Show, Next, clears, overlays on/off) and the recovery writer **still writes `live-state.json`** in read-only mode (spec §4.7). The operator shows a banner (Plan 3 notice line).

- [ ] **Step 1: Write the failing tests** (`store::tests` and `app::tests`): `migration_saves_immediately_and_does_not_rerun`, `migration_makes_labeled_backup_first`, `migration_with_missing_asset_still_loads`, `labeled_backup_survives_prune` (store level), `newer_format_opens_read_only`, `newer_format_unparseable_still_starts_read_only`, `syntax_error_is_still_could_not_be_read`, `after_load_is_the_only_hook_and_saves_when_it_returns_true`, `migration_chain_backs_up_before_each_step`, `read_only_rejects_edits`, `read_only_still_writes_live_state` (read-only app, Show a slide → `live-state.json` is written and `data.json` is not), `read_only_keeps_live_control` (Show/Next/F1 work and return Ok).
- [ ] **Step 2: Run** `cargo test --locked --lib store::` and `cargo test --locked --lib app::tests::read_only`. Expected: FAIL (`cannot find value `SUPPORTED_FORMAT``, `no method named `backup_labeled``).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the two commands, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(store): format 2, migration chain, newer-data guard, labelled backups (FEAT-1 B2)`.

### Task B3: Slides carry styles

**Files:** Modify `src/slides.rs`, `src/model.rs` (`SlideContent::Text.styles`).

`SlideContent::Text` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] styles: Vec<StyleSpan>`. `text_slides(title, body, styles)` uses `richtext::blocks`. Scripture and song slides get empty `styles`.

- [ ] **Step 1: Write the failing tests** (`slides::tests`): `text_slides_carry_sliced_styles`, `text_slides_without_styles_unchanged`.
- [ ] **Step 2: Run** `cargo test --locked --lib slides::`. Expected: FAIL (`no field `styles``).
- [ ] **Step 3: Implement**; fix the `text_slides` call sites.
- [ ] **Step 4: Run** `cargo test --locked --lib slides::`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(slides): announcement slides carry rich text styles (FEAT-1 B3)`.

### Task B4a: Live engine — theme snapshot and `LiveView.theme`

**Files:** Modify `src/live.rs`, `src/app.rs` (`DataPlan`, and the `LiveView` fill that today sets `theme` from `data.settings.theme`).

**Interfaces** (spec §4.2, §4.6):
- `LiveItem.theme: Theme`, filled by `DataPlan::build` from `theme::resolve` with `theme::service_of`.
- `SlideLayer::Text { …, #[serde(default)] theme: Option<Theme>, #[serde(default, skip_serializing_if = "Vec::is_empty")] styles }`. `go()` sets `theme: Some(item.theme.clone())` and the slide's `styles`.
- **`LiveView.theme` keeps meaning "the church default's style"**: `app.rs` fills it with `theme::church_default(&data).style` (not `data.settings.theme`, which is Standard). Stage and old consumers therefore follow the starred theme.

- [ ] **Step 1: Write the failing tests** (`live::tests`, `app::tests`): `live_item_snapshots_resolved_theme`, `theme_edit_does_not_change_frame`, `show_after_theme_change_applies_new_theme`, `next_within_item_keeps_snapshot_theme`, `live_view_theme_is_church_default_style` (star "Christmas" → `LiveView.theme == christmas.style`; star Standard → `settings.theme`), `old_live_state_json_loads` (a `SlideLayer::Text` without `theme`/`styles` loads with `None`/empty).
- [ ] **Step 2: Run** `cargo test --locked --lib live::` and `cargo test --locked --lib app::tests::live_view_theme`. Expected: FAIL (`no field `theme` on type `LiveItem``).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both commands, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(live): snapshot the resolved theme per live item (FEAT-1 B4a)`.

### Task B4b: Live engine — overlay commands, slots, clear epoch and `inst`

**Files:** Modify `src/live.rs`, `src/app.rs` (`DataPlan` overlay lookups), Plan 4's `recovery::state` (persisted fields).

**Interfaces** (spec §4.5, §5.2):
- `Frame.overlays: Vec<OverlaySlot>` (`default, skip_serializing_if empty`).
- `Command::Overlay { id, on, #[serde(default)] seen_clear: Option<u64> }` and `Command::ClearOverlays`.
- `Live.clear_epoch: u64` (`#[serde(default)]`, persisted, exposed on `LiveView`). `Live.next_inst: u64` (`#[serde(default)]`, persisted so a restart never reuses an `inst`); each overlay show takes `next_inst` and increments it.
- The private mutators `show_overlay`, `hide_overlay` and `retain_overlays` delegate to `overlay::slots::{show, hide, retain}` and do not bump the version.
- `Plan::saved_overlay(id) -> Option<(u32, Overlay)>` and `Plan::logo() -> Option<AssetId>`.
- `apply` follows the matrix: on → `check_seen_clear` + `saved_slot(.., inst = next_inst)` + `turn_on`; off → `hide`; `ClearOverlays` → retain none; `ClearAll` → `Frame::default()`; `Logo` → `retain(survives_logo)`. Every F1–F6 clear and Logo bumps `clear_epoch`, even when nothing was removed. A version bump happens only on a change. Overlay commands need no `base`.

- [ ] **Step 1: Write the failing tests** (`live::tests`): `overlay_on_bumps_and_is_idempotent`, `overlay_on_changed_copy_replaces`, `overlay_off_when_absent_is_noop`, `ninth_saved_overlay_is_rejected`, `cap_ignores_non_saved_slots`, `overlays_survive_next_show_and_clears_except_all`, `logo_keeps_text_and_clock`, `clear_all_removes_overlays`, `clear_overlays_noop_when_none`, `overlay_commands_need_no_base`, `overlay_on_with_stale_seen_clear_is_refused`, `overlay_on_without_seen_clear_is_refused`, `overlay_off_and_clear_overlays_ignore_seen_clear`, `every_clear_and_logo_bumps_clear_epoch_even_when_nothing_removed`, `clear_epoch_survives_restore`, `inst_increments_and_survives_restore` (two shows get inst n and n+1; after a save/restore the next show gets n+2), `deleted_overlay_stays_on_screen_until_off`, `show_overlay_and_hide_overlay_do_not_bump`, `logo_bug_follows_logo_change_on_next_toggle`, `logo_bug_without_logo_errors`. (`logo_removes_timer_keeps_bug_and_message` lands with FEAT-2's arms.)
- [ ] **Step 2: Run** `cargo test --locked --lib live::`. Expected: FAIL (`no variant `Overlay` in `Command``, `no field `clear_epoch``).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked --lib live::`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(live): overlay slots, overlay commands, clear epoch (FEAT-1 B4b)`.

### Task B4c: Live engine — `OverlayFailed` and the notice

**Files:** Modify `src/live.rs`, `src/room.rs` (`RoomEvent`).

**Interfaces** (spec §5.3):
- `RoomEvent::OverlayFailed { key, inst, reason }`. `Live::report` calls `slots::failed`; when it returns a name, bump the version and set `live.notice = strings::themes::overlay_failed(name, reason)`. `notice` is cleared on the next `Visual` change. A failed overlay never blocks Next or restore.

- [ ] **Step 1: Write the failing tests** (`live::tests`): `overlay_failed_removes_slot_and_sets_notice`, `stale_overlay_failed_report_ignored`, `missing_overlay_picture_does_not_block_next`, `restore_with_missing_overlay_asset_shows_slides`, `overlay_notice_clears_on_next_visual_change`.
- [ ] **Step 2: Run** `cargo test --locked --lib live::`. Expected: FAIL (`no variant `OverlayFailed``).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked --lib live::`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(live): overlay failure reports and notice (FEAT-1 B4c)`.

### Task B5a: Renderer — fonts and rich runs

**Files:** Modify `src/render/layout.rs`, `src/render/raster.rs`, `tests/render_golden.rs`; add new PNGs in `tests/golden/`.

**Interfaces:**
- `Fonts::new` and `Fonts::bundled_only` call `paint::fonts::add_rich_faces` then `add_extra_faces`.
- `TextSlide.styles: &[StyleSpan]`. `layout` shapes with `paint::runs::{run_specs, shape_runs, reshape}` (weight `theme.weight`, scripture 500). `raster` paints per-run colour with `paint_glyphs` and underlines with `underline_rects`.

- [ ] **Step 1: Write the failing goldens/tests:** goldens `default_theme_pixels_unchanged` (the five existing goldens, untouched), `bold_italic_underline_runs`, `bold_run_in_semibold_theme_is_700`, `colored_run`, `large_run_still_fits`; unit test `render::layout::tests::fonts_new_loads_rich_and_extra_faces`.
- [ ] **Step 2: Run** `cargo test --locked --test render_golden`. Expected: FAIL (new PNGs missing; `no field `styles` on `TextSlide``).
- [ ] **Step 3: Implement.** Then generate **only the new names**: in PowerShell `$env:UPDATE_GOLDENS = "1"; cargo test --locked --test render_golden bold_italic_underline_runs bold_run_in_semibold colored_run large_run_still_fits; Remove-Item Env:UPDATE_GOLDENS`. Open every new PNG and check it by eye.
- [ ] **Step 4: Run** `cargo test --locked --test render_golden`, then `cargo test --locked`. Expected: PASS; the five old goldens are byte-identical (`git status tests/golden` shows only the new files).
- [ ] **Step 5: Commit** `feat(render): bold, italic and extra faces; rich runs with colour and underline (FEAT-1 B5a)`.

### Task B5b: Renderer — alignment, position top, uppercase with styles

**Files:** Modify `src/render/layout.rs`, `tests/render_golden.rs`; new PNGs.

**Interfaces:** `layout` uses `theme.align` (scripture always Left), `position == "top"`, and `richtext::uppercase` when `uppercase && !scripture` (spans shift with the uppercased text).

- [ ] **Step 1: Write the failing goldens/tests:** goldens `align_left_right`, `position_top`; unit test `render::layout::tests::uppercase_keeps_run_colours` (a gold span stays on the same word after uppercasing "ﬁne gold").
- [ ] **Step 2: Run** `cargo test --locked --test render_golden` and `cargo test --locked --lib render::layout`. Expected: FAIL (PNGs missing; alignment ignored).
- [ ] **Step 3: Implement**, then `UPDATE_GOLDENS=1` for `align_left_right position_top` only, and look at both PNGs.
- [ ] **Step 4: Run** both commands, then `cargo test --locked`. Expected: PASS; old goldens unchanged.
- [ ] **Step 5: Commit** `feat(render): text alignment, top position and styled uppercase (FEAT-1 B5b)`.

### Task B5c: Renderer — outline, text band and background goldens

**Files:** Modify `src/render/raster.rs`, `tests/render_golden.rs`; new PNGs.

**Interfaces:** Outline is `paint::decor::dilate` of the body coverage in `outline_color`, drawn under the text and above the shadow. The band is `band_box` + `fill_round_rect`, drawn first, and replaces the lower-third shade when `position == "lower"`. Backgrounds in goldens go through `render::background::background_pixels`.

- [ ] **Step 1: Write the failing goldens/tests:** goldens `outline_thick`, `text_band_lower`, `gradient_background_135`, `dim_50_image_background`; unit test `render::raster::tests::band_replaces_lower_third_shade` (with `band: true, position: "lower"`, the shade pixels above the band are the background colour).
- [ ] **Step 2: Run** `cargo test --locked --test render_golden` and `cargo test --locked --lib render::raster`. Expected: FAIL.
- [ ] **Step 3: Implement**, then `UPDATE_GOLDENS=1` for the four new names only, and look at each PNG.
- [ ] **Step 4: Run** both commands, then `cargo test --locked`. Expected: PASS; old goldens unchanged.
- [ ] **Step 5: Commit** `feat(render): outline, text band, gradient and darken goldens (FEAT-1 B5c)`.

### Task B5d: Renderer — `CacheKey`, overlay job and overlay shadow

**Files:** Modify `src/render/cache.rs`, `src/render/raster.rs`, `src/render/overlay.rs` (moved in B1), `tests/render_golden.rs`; new PNGs.

**Interfaces:**
- `RenderKey.styles: Vec<StyleSpan>`. New `pub enum CacheKey { Slide(RenderKey), Overlay(OverlayKey), Stage(StageKey) }` (`StageKey` is a placeholder unit struct until FEAT-3). One LRU holds all three. `Job::Overlay(OverlayKey)`.
- `raster` exposes `pub(crate) fn shadow_into(pix, buf, at, scale)`; `render::overlay::draw` uses it when `look.shadow`.

- [ ] **Step 1: Write the failing goldens/tests:** goldens `overlay_text_bar_bottom`, `overlay_picture_top_right_scaled`, `overlay_clock_box`, `overlay_on_4x3_keeps_corner`, `overlay_draw_is_bounding_box_only`; unit tests `render::cache::tests::cache_key_keeps_slide_and_overlay_entries_apart`, `render::cache::tests::render_key_includes_styles`, `render::overlay::tests::overlay_shadow_follows_look_shadow`.
- [ ] **Step 2: Run** `cargo test --locked --test render_golden` and `cargo test --locked --lib render::`. Expected: FAIL (`cannot find type `CacheKey``).
- [ ] **Step 3: Implement**, then `UPDATE_GOLDENS=1` for the five overlay names only, and look at each PNG.
- [ ] **Step 4: Run** both commands, then `cargo test --locked`. Expected: PASS; old goldens unchanged.
- [ ] **Step 5: Commit** `feat(render): one cache for slides and overlays, overlay shadow (FEAT-1 B5d)`.

### Task B6: Room, live preview and thumbnails draw overlays and new backgrounds

**Files:** Modify `src/room.rs`, `src/ui/room.rs`, `src/ui/room/prepare.rs`, `ui/room.slint`, `src/ui/preview.rs` (Plan 3 Task 9 `LiveMonitor`), Plan 3's thumbnail renderer.

**Interfaces** (spec §5.3, §6.1, §6.3, §6.5):
- `Kind::Overlays` and `LayerKey::Overlays(String)` (JSON of the slots without `inst`, plus the canvas), with timeout `STILL`.
- **Slide key:** `room.rs Layers::of` keys the slide layer on `SlideLayer::Text.theme.unwrap_or(view.theme)` (the slide's own snapshot, falling back to `LiveView.theme` only for old `live-state.json`), so editing the church default never replans a slide that carries its snapshot.
- Prepare draws each slot with `render::overlay::draw` on its own and returns `Vec<Result<PlacedPic, (key, inst, reason)>>`. Each error sends `RoomEvent::OverlayFailed` and never fails the plan.
- `OverlayPic { key, image, x, y, w, h, shown }` is drawn by a repeater above `logo` and below `black`, with a 400 ms opacity fade (`OVERLAY_FADE_MS`) and a crossfade on in-place change.
- A 1 Hz `slint::Timer` runs while a Clock slot is on and swaps only that image (no commit).
- The background `FadeLayer` uses `render::background::background_pixels`, plus a dim rectangle over video.
- `LiveMonitor` gets the same inputs. **Plan 3 thumbnails** draw their background with `background_pixels` too (gradients and Darken show in the slide grid), and never show overlays.

- [ ] **Step 1: Write the failing tests** (`room::tests`, `ui` tests): `overlay_change_prepares_only_overlays`, `failing_overlay_slot_reports_and_commits_other_layers`, `church_default_edit_does_not_replan_slide_with_snapshot_theme`, `clock_tick_does_not_need_a_plan`, `slide_layer_keys_on_its_own_theme_else_view_theme`, `thumbnails_draw_gradient_backgrounds` (a Midnight background thumbnail's top-left pixel ≈ `#0b1a33`).
- [ ] **Step 2: Run** `cargo test --locked --lib room::` and `cargo test --locked --lib ui::`. Expected: FAIL (`no variant `Overlays` in `Kind``).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both commands, then `cargo test --locked`. Expected: PASS. Then manual checks 5, 6 and 8 from spec §9.3; note the results for B14.
- [ ] **Step 5: Commit** `feat(room): overlay layer, per-slot prepare, gradients and darken (FEAT-1 B6)`.

### Task B7a: Core actions — themes

**Files:** Modify `src/app.rs`.

**Interfaces** (spec §5.1): `theme/save` → `{id, screen_views: []}` via `validate_save` (an id of `STANDARD_ID` is refused, plan decision 13); `theme/save-standard` writes only `settings.theme` and `settings.background`; `theme/make-default` via `default_ref`; `theme/delete` via `delete_kind` (clears every `Service.theme`/`ServiceItem.theme` reference and `default_theme` in the same `edit`) → `{items, services, was_default}`; `theme/usage` → `{items, services, is_default}`. Every action goes through `App::edit`, so FEAT-4's reconcile (`LookOnScreen`) runs after it.

- [ ] **Step 1: Write the failing tests** (`app::tests`): `theme_save_requires_unique_name`, `theme_name_church_default_reserved`, `theme_save_rejects_standard_id`, `theme_save_returns_empty_screen_views`, `theme_delete_clears_references_and_counts`, `delete_starred_theme_returns_to_standard`, `make_default_keeps_previous_default_as_theme`, `editing_starred_theme_changes_default`, `theme_save_standard_writes_only_theme_and_background`, `theme_change_on_live_item_sets_look_on_screen`, `theme_change_on_item_no_longer_found_sets_nothing` (the last two need FEAT-4's reconcile; mark them `#[ignore = "needs FEAT-4 reconcile"]` if FEAT-4 has not merged yet).
- [ ] **Step 2: Run** `cargo test --locked --lib app::tests::theme`. Expected: FAIL (`unknown action theme/save`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command, then `cargo test --locked`. Expected: PASS (ignored tests reported as ignored).
- [ ] **Step 5: Commit** `feat(app): theme actions (FEAT-1 B7a)`.

### Task B7b: Core actions — service theme and merge-by-key saves

**Files:** Modify `src/app.rs`.

**Interfaces:** `service/theme` (unknown id → `THEME_GONE`; result `{own_background: n}` counting items of that service with their own background); `service/clear-item-backgrounds` (that service only); `service/update` via `merge::merge_service`, then for each text item: body normalised to `\n` line ends (`"\r\n"` and lone `"\r"` → `"\n"`) with its spans remapped to the new offsets, then `richtext::normalize(body, styles)` (spec §4.4); `settings` via `merge::merge_typed(.., merge_object)` (this replaces Plan 3's copy list).

- [ ] **Step 1: Write the failing tests** (`app::tests`): `service_theme_rejects_unknown`, `service_theme_reports_items_with_own_background`, `clear_item_backgrounds_only_that_service`, `service_update_normalizes_styles`, `service_update_without_theme_key_keeps_theme`, `service_update_without_styles_key_keeps_styles`, `service_update_missing_keys_keep_every_new_field`, `text_body_saved_with_lf_and_spans_remapped` (body `"Hi\r\nyou"` with bold on "you" (4..7) is saved as `"Hi\nyou"` with bold 3..6), `settings_save_keeps_logo_set_meanwhile`, `settings_save_keeps_theme_background_changed_meanwhile`, `settings_save_missing_key_keeps_value`, `settings_save_null_clears_optional_field`.
- [ ] **Step 2: Run** `cargo test --locked --lib app::tests::service_` and `cargo test --locked --lib app::tests::settings_save`. Expected: FAIL.
- [ ] **Step 3: Implement.** (The `\r\n` remap reuses the same rule as `richtext::blocks`; put it in `richtext` as `pub fn to_lf(text, spans) -> (String, Vec<StyleSpan>)` with a unit test `to_lf_remaps_spans`.)
- [ ] **Step 4: Run** both commands and `cargo test --locked --lib richtext::`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): service theme actions and merge-by-key saves (FEAT-1 B7b)`.

### Task B7c: Core actions — backgrounds, import purpose and conversion target

**Files:** Modify `src/app.rs`, `src/import/media.rs` (as Plan 4 left it).

**Interfaces:** `background/add` (from a built-in or a custom gradient), `background/rename` (blank → `BACKGROUND_NAME_REQUIRED`), `background/remove` (unknown → `BACKGROUND_GONE`; items, themes and settings that use the value keep it, the picker then shows it as Current). `ImportPurpose::Background` adds a `BackgroundEntry` named `file_stem_name`, or fails with `BACKGROUND_KIND`. If Plan 4 still converts some videos, `ConvertTarget::{Media(Uuid), Background(Uuid)}` routes progress and the final asset swap; if Plan 4 removed conversion, entries are always `Ready` and `background_video_needing_conversion_becomes_ready` checks that instead.

- [ ] **Step 1: Write the failing tests** (`app::tests`): `background_import_purpose_adds_entry_not_media`, `background_import_rejects_non_picture_or_video`, `background_video_needing_conversion_becomes_ready`, `background_rename_blank_is_rejected`, `background_remove_keeps_values_in_use`, `background_remove_unknown_says_gone`.
- [ ] **Step 2: Run** `cargo test --locked --lib app::tests::background`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): background library actions and background import (FEAT-1 B7c)`.

### Task B7d: Core actions — overlays and logo

**Files:** Modify `src/app.rs`.

**Interfaces:** `overlay/save` via `overlay::validate` (asset check against `data.media`/`backgrounds`); `overlay/delete` (library only; a slot on screen stays until turned off); `overlay/move` (new index; order = drawing order); `settings/logo` (`Some(asset)`/`None`, saved immediately, toast `LOGO_SAVED`/`LOGO_REMOVED`).

- [ ] **Step 1: Write the failing tests** (`app::tests`): `overlay_save_validates`, `overlay_delete_keeps_slot_on_screen`, `overlay_move_reorders_library`, `logo_choose_saves_immediately`, `logo_remove_saves_immediately`.
- [ ] **Step 2: Run** `cargo test --locked --lib app::tests::overlay` and `cargo test --locked --lib app::tests::logo`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both commands, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): overlay library and logo actions (FEAT-1 B7d)`.

### Task B8: Settings → Themes tab and the Theme editor

**Files:** Create `ui/dialogs/settings-themes.slint`, `src/ui/dialogs/settings_themes.rs`, `ui/dialogs/theme-editor.slint`, `src/ui/dialogs/theme_editor.rs`. Modify Plan 3's Settings dialog (Task 20): the Look tab becomes Themes, and the tab list follows spec §7.5. Each tab sends only its own fields.

Rows come from `Catalog::rows`. Star → confirm `make_default_question` → `theme/make-default` → toast `now_default`. Delete/Reset → `theme/usage` → `delete_theme_question`. New and Duplicate names use `NEW_THEME_NAME` and `copy_name`. The editor has the two columns of spec §3.3. **Font list:** the families in `paint::fonts::BUNDLED_FAMILIES` first, each suffixed with `BUNDLED_FONT_NOTE` ("works on every computer"), then the 20 web fonts, then "More fonts on this computer…". Then Weight; sizes; Align; Position; ALL CAPS; Shadow; Outline; Text band; Background picker in SavedTheme or StandardTheme mode. The preview re-renders 150 ms after each change with `render_text` + `background_pixels`. The `font_missing` note shows when needed. Logo buttons call `settings/logo`.

- [ ] **Step 1: Write the failing UI tests:** `themes_tab_lists_standard_first_with_star`, `new_theme_saves_and_appears`, `star_asks_then_moves_default`, `delete_theme_asks_with_counts`, `theme_editor_preview_rerenders_after_change`, `missing_font_note_in_theme_editor`, `font_list_starts_with_bundled_families` (first four rows are Noto Sans, Noto Serif, Montserrat, Oswald, each with the note), `logo_section_saves_without_settings_save`, `settings_dialog_sends_only_edited_fields`.
- [ ] **Step 2: Run** `cargo test --locked --test ui_operator_flows themes_tab` (and the other names). Expected: FAIL (elements missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked --test ui_operator_flows`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): Themes tab and theme editor (FEAT-1 B8)`.

### Task B9: Background picker sections and the Backgrounds manager

**Files:** Modify `ui/dialogs/background-picker.slint`, `src/ui/dialogs/background.rs` (Plan 3 Task 18). Create `ui/dialogs/backgrounds.slint`, `src/ui/dialogs/backgrounds.rs`.

`choices()` and `selected()` become thin adapters over `theme::picker`. Add the Custom gradient control (two colours + angle 0/45/90/135/180), the Darken slider (0–80) with help `DARKEN_HELP` when `darken_shown`, and "Add picture or video…" (rfd, multi-select, `ImportPurpose::Background`, toast `ADDED_TO_BACKGROUNDS`). The manager has Rename, Remove (`remove_background_question`), a search box above 24 entries, and file drop that imports as backgrounds.

- [ ] **Step 1: Write the failing UI tests:** `background_picker_sections_and_darken_slider`, `picker_keeps_removed_library_background`, `converting_background_not_pickable`, `add_background_imports_to_library`, `backgrounds_manager_renames_and_removes`.
- [ ] **Step 2: Run** `cargo test --locked --test ui_operator_flows background`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked --test ui_operator_flows`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): sectioned background picker and Backgrounds manager (FEAT-1 B9)`.

### Task B10: Rich text editor, Item edit sections and keys

**Files:** Create `ui/rich-edit.slint`, `src/ui/rich_edit.rs`. Modify `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs` (Plan 3 Task 18) and `src/ui/operator/keys.rs` (Plan 3 Task 10).

- `RichEdit` = toolbar + `TextInput` in a `ScrollView` (8 lines) + Formatting chips + preview strip (250 ms). All state lives in `theme::rich_edit::RichEditModel`. `edited` and selection come from `cursor-position-byte-offset` / `anchor-position-byte-offset`, and the toolbar buttons from `pressed`/`bold_disabled` (tooltip `BOLD_DISABLED_TIP`).
- Item edit becomes scrollable with sections Words, Theme, Automation, Stage and Bible (only the ones that apply) and fits in 768 px height. Its Theme section has the Theme combo ("Same as service (<name>)" + rows) and the picker in ItemEdit mode.
- `keys.rs`: `F6` → `KeyAction::Clear(Command::ClearOverlays)` in the capture set. `KeyAction::PassThrough` handles Ctrl+B with a dialog open (the callback returns false). Plan 3's `ctrl_b_ignored_while_a_dialog_is_open` is renamed and rewritten. Help gets "F6 — Clear overlays".

- [ ] **Step 1: Write the failing tests:** `keys::tests`: `ctrl_b_passes_through_while_a_dialog_is_open`, `f6_clears_overlays_in_capture_set`; UI: `item_edit_theme_combo_saves`, `item_edit_fits_768_height`, `rich_edit_bold_selection_then_save_round_trips`, `rich_edit_lists_formatted_ranges`, `rich_edit_chip_x_clears_range`, `rich_edit_ctrl_b_bolds_in_dialog_not_bible_box`, `rich_edit_b_disabled_in_bold_theme`, `rich_edit_pending_style_applies_to_typed_text`, `f6_clears_overlays_while_typing`.
- [ ] **Step 2: Run** `cargo test --locked --lib ui::operator::keys` and `cargo test --locked --test ui_operator_flows rich_edit`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): rich text editor, Item edit sections, F6 and Ctrl+B pass-through (FEAT-1 B10)`.

### Task B11: Service theme chip, Services dialog row and Copy

**Files:** Modify `ui/operator/service.slint`, `src/ui/operator/service.rs` (Plan 3 Task 6), `src/ui/operator/view.rs` (subtitle: `OWN_BACKGROUND_SUFFIX`, " · <theme name>"), `ui/dialogs/services.slint` and its glue (Task 19), and the service-copy action in `src/app.rs`.

The chip shows `theme_chip(name)` and opens a menu: Church default (<name>), the rows, and Manage themes…. Picking runs `service/theme`. When the result's `own_background > 0`, the operator is asked `own_backgrounds(n)`, with **Use theme background** (`service/clear-item-backgrounds`) and **Keep theirs**. The Services dialog row shows the theme name after "N item(s)" when it is not the church default. **Copy** of a service keeps its `theme` (spec §3.2 step 5).

- [ ] **Step 1: Write the failing tests:** UI `service_theme_chip_picks_theme`, `service_theme_asks_about_own_backgrounds`, `services_dialog_row_shows_theme_name`; view `subtitle_shows_own_background_and_theme`; `app::tests::copy_service_keeps_theme`.
- [ ] **Step 2: Run** `cargo test --locked --test ui_operator_flows service_theme` and `cargo test --locked --lib app::tests::copy_service_keeps_theme`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): service theme chip, own-background question, Services row theme (FEAT-1 B11)`.

### Task B11b: Narrow-window rules and scrollable Services dialog (layout budget, spec §7.2)

FEAT-1 owns the single layout budget (spec §7.2, decision 35). The Live-column rows are placed by B12 (Overlays strip), FEAT-2/3/4 (their strips); this task does the rest of §7.2 that maps to no other task.

**Files:** Modify Plan 3's top bar (`ui/operator/top-bar.slint` or wherever Plan 3 Task 6/10 put it, and its glue), Plan 3's slides header (`ui/operator/slides.slint` header and its glue), `ui/dialogs/services.slint` and its glue (Task 19).

**Interfaces:**
- A `narrow: bool` property on the operator window, true when the window width is **below 1500 px**.
- Top bar: when `narrow`, **Automation**, **Practice** and **⋯ More** show icon only, with their labels as tooltips (buttons that don't exist yet because FEAT-2/FEAT-6 have not merged are simply absent; their plans read `narrow`).
- Slides header: when `narrow`, **Choose/Print**, **Slide breaks** and **Custom order** collapse into one **⋯** menu with the same actions (buttons not yet present are left out).
- Services dialog: content in a `ScrollView` with sections, fitting a 768 px tall window.
- FEAT-5's second-translation chip folding and FEAT-3's single **Stage…** button at ≤ 1250 px are those plans' work; they read the same `narrow` property.

- [ ] **Step 1: Write the failing UI tests:** `top_bar_icons_only_below_1500px` (window 1499×900 → the More button has no visible text and a tooltip; 1500×900 → text visible), `slides_header_folds_into_more_below_1500px`, `services_dialog_fits_768_height` (1366×768 → the dialog's bottom buttons are inside the window and its list scrolls).
- [ ] **Step 2: Run** `cargo test --locked --test ui_operator_flows below_1500px` and `cargo test --locked --test ui_operator_flows services_dialog_fits`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked --test ui_operator_flows`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): narrow-window top bar and slides header, scrollable Services dialog (FEAT-1 B11b)`.

### Task B12: Overlays strip, manager and editor

**Files:** Create `ui/operator/overlays.slint`, `src/ui/operator/overlays.rs`, `ui/dialogs/overlays.slint`, `ui/dialogs/overlay-editor.slint`, `src/ui/dialogs/overlays.rs`. Modify the Live column (Plan 3 Task 9) to place the strip at row 5 of the spec §7.2 budget.

- The strip is 36 px high and scrolls horizontally. Chips come from `slots::chips`; each is 28 px tall and at most 140 px wide with ellipsis, with ids `overlay-chip-<n>`. `overlays-clear-btn` (F6) and `overlays-add-btn` are pinned at the right. With no saved overlays the strip collapses to "+ Overlays".
- A changed chip shows a dot with tooltip `OVERLAY_CHANGED_TIP`. A deleted chip shows a struck-through name.
- Every overlay-on sends `seen_clear` from the newest `LiveView.clear_epoch`. A `SCREEN_CLEARED` refusal becomes a toast.
- The manager offers Edit, Duplicate, Delete and drag-to-reorder (`overlay/move`). The editor follows spec §3.6 step 4. Its preview draws the overlay over a sample lyric on the church default background with `render::overlay::draw`. The Text kind's help line is `TEXT_OVERLAY_HELP`, or `TEXT_OVERLAY_HELP_FEAT2` once FEAT-2 has merged (FEAT-2's plan switches the constant).

- [ ] **Step 1: Write the failing tests:** `overlay_chip_toggles_and_lights`, `overlays_strip_collapses_when_empty`, `overlay_edit_while_on_shows_changed_dot`, `overlay_failed_darkens_chip_and_shows_notice`, `overlays_strip_fits_live_column_budget_at_1366x768`, `text_overlay_kind_shows_help_line`, `theme_change_on_live_item_shows_feat4_banner_and_update_screen_applies_it` (needs FEAT-4; `#[ignore]` until it merges).
- [ ] **Step 2: Run** `cargo test --locked --test ui_operator_flows overlay`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked --test ui_operator_flows`, then `cargo test --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): overlays strip, manager and editor (FEAT-1 B12)`.

### Task B13: Phone remote

**Files:** Modify `src/server.rs` (`remote_command`, `remote_view`), `web/remote.html`, `web/remote.js` (the phone page Plan 4 keeps).

The phone allowlist adds `Overlay { id, on }` (on requires `seen_clear`) and `ClearOverlays`, following FEAT-2 §5.11. A fresh `base` is required for Show, Next, Prev, GoToCue, TakeLive and Jump. `remote_view.overlays = slots::remote_list(..)`. The page shows chips and "Clear overlays", sends `seen_clear`, and shows `SCREEN_CLEARED` on refusal.

- [ ] **Step 1: Write the failing tests** (`server::tests`): `remote_may_toggle_overlays`, `remote_overlay_on_with_old_seen_clear_says_screen_was_cleared`, `remote_next_prev_gotocue_takelive_need_base`, `remote_may_not_save_overlays_or_themes`, `remote_view_lists_saved_overlays_with_state`.
- [ ] **Step 2: Run** `cargo test --locked --lib server::`. Expected: FAIL.
- [ ] **Step 3: Implement** (server and phone page).
- [ ] **Step 4: Run** `cargo test --locked --lib server::`, then `cargo test --locked`. Expected: PASS. Then open the phone page against a running app and toggle one overlay by hand.
- [ ] **Step 5: Commit** `feat(remote): overlay toggles and clear on the phone (FEAT-1 B13)`.

### Task B14: Manual checks and document updates

**Files:** Modify `PROJECT_PLAN.md` (§10b FEAT-1 row, reworded as spec §11 step 11 says), `docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md` (§6.2 F6 column, per-source overlay rows and the Logo bullet; §6.3; §7.3 → FEAT-1 §7.6; §12.3 → FEAT-2 §5.11), `docs/superpowers/checks/2026-native-checks.md`.

- [ ] **Step 1:** Run manual checks 1–11 from spec §9.3 on Windows (and on macOS/Linux when available). Record the results in `docs/superpowers/checks/2026-native-checks.md` under "FEAT-1".
- [ ] **Step 2:** Make the document edits.
- [ ] **Step 3:** Run `cargo test --locked`. Expected: PASS.
- [ ] **Step 4:** Commit `docs: FEAT-1 manual checks and spec/plan updates (FEAT-1 B14)`.
