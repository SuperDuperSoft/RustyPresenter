# FEAT-2 — Service Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Timers (countdown to a time, countdown of a length, count up), message templates with `{fields}`, macros, slide auto-advance with announcement loops, and the pre-service countdown. Timers tick in core, not in the UI. Everything goes through the live engine's safety rules: nothing visual goes live by itself unless the operator set it up and nobody has changed the screen since, and an intentional clear always wins.

**Architecture:** The work is in two parts so it merges cleanly with the native rewrite (Plans 3 and 4), which is changing `app.rs`, `live.rs`, `store.rs`, `server.rs`, `src/ui/**` and `ui/**` right now.
- **Part A (buildable now)** creates only new files: `src/lenient.rs`, `src/timers.rs`, `src/strings/automation.rs` and `src/automation/{mod,messages,macros,advance,preservice,validate,slots,runtime,ticker,remote,strip}.rs`. They hold pure logic with unit tests: the persisted types with serde defaults, the timer state machine, the formatter, time-of-day targets (generic over `chrono::TimeZone`), message parsing and filling, macro classification and resolution, the auto-advance state machine, the pre-service builder and end rule, seeding and validation, the runtime bookkeeping, the ticker's publish rule and the operator strip rules. Every function takes its inputs explicitly (`now`, the time zone, the service start time, the clear epoch, the live cursor as a plain struct), so nothing touches `App`, `Live` or `Data`. The only existing file touched is `src/lib.rs`, which gets three lines (`pub mod lenient;`, `pub mod timers;`, `pub mod automation;`). `src/automation/mod.rs` reaches the strings file at its final spec path with one `#[path]` line.
- **Part B (blocked until `native-foundation` has Plans 3 and 4, and FEAT-1's overlay layer)** wires Part A in: model fields, the `live.rs` mutate/commit split and `apply_batch`, the new commands and `InternalCommand`, the App runtime and actions, the ticker task and `Update::Timers`, the overlay render arms, the operator strip, the dialogs and the phone remote. It is written against Plan 3's file layout.

**Tech Stack:** Rust 2024; `chrono 0.4.45` (already in `Cargo.toml` with `default-features = false, features = ["clock"]`); `serde`/`serde_json`; `uuid` (v4 + serde). Part B adds Slint 1.18 UI (Plan 3), `tiny-skia`/`cosmic-text` drawing through FEAT-1's `theme::paint::overlay::draw` (`src/theme/paint/overlay.rs`), and the axum phone remote. **No new crates.** The spec's dev-dependency `chrono-tz` is replaced by a 40-line test-only `TimeZone` (Task A4), so `Cargo.toml` is not touched.

**Spec:** `docs/superpowers/specs/2026-11-01-feat2-service-automation-design.md` (FEAT-2), all sections. Depends on `2026-11-01-feat1-themes-backgrounds-props-design.md` §4.5 (overlay types, `Placement`, choice-list mapping), §4.7 (`Data::after_load`), §5.2 (clear matrix, `clear_epoch`), §6.1 (compose order), §7 (key table). The FEAT-3, FEAT-4, FEAT-6 and FEAT-7 hooks are in spec §10. v1 spec §4.3, §5–§7 (live safety) win any conflict.

## Global Constraints

- **Part A touches only new files** plus three lines in `src/lib.rs`: `pub mod lenient;` (A1), `pub mod timers;` (A2) and `pub mod automation;` (A1). Three lines instead of one is safe: each declares a module whose file is new, so no existing item, path or test changes, and the lines merge as plain additions next to Plan 3/4's module list. It does not edit `src/app.rs`, `src/live.rs`, `src/model.rs`, `src/store.rs`, `src/room.rs`, `src/slides.rs`, `src/server.rs`, `src/bible*`, `src/ui/**`, `ui/**`, `web/**` or `Cargo.toml`.
- **Final paths now (decided with FEAT-3, Plan decision 17).** `lenient` and `timers` are crate-root modules from the start: `crate::lenient::Lenient` and `crate::timers::{TimersView, TimerSnapshot, TimerState, TimerKind, text, …}`. FEAT-3 Part A imports exactly these paths, and its Global Constraints record the same decision. Only the strings file uses `#[path]`: `src/automation/mod.rs` declares `#[path = "../strings/automation.rs"] pub mod strings;`. Task B1 swaps that line for `pub use crate::strings::automation as strings;` without moving the file.
- **Paths inside Part A code.** Code names these modules only as `crate::lenient::Lenient`, `crate::timers::…` and `crate::automation::strings::…` (never `super::strings` or `self::`-relative paths to them). All three paths still resolve after B1.
- **Declare before you test.** Each task's Step 1 adds the module declaration (`src/lib.rs` line or `src/automation/mod.rs` line) together with the tests, so Step 2 really fails to compile. Each Step 4 names the expected `running N tests` count, so a filter that matches nothing can't pass silently.
- **Test command (Part A), PowerShell:** `$FC = "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd"`, then `& $FC test --release --lib <module path>`, e.g. `& $FC test --release --lib timers::` or `& $FC test --release --lib automation::messages`. Part B uses the repo's normal `cargo test` after the merge; more than one filter goes after `--` (`cargo test -- a:: b::`).
- **Forward compatibility (spec §4.1).** Every persisted struct has `#[serde(default)]` and a `Default` impl. `MacroStep`, `TimerKind` and `AtZero` are stored as `Lenient<…>`. Every unit-only enum (`Overrun`, `OverlayPlace`, `MessagePlace`, `MacroColor`, `PreServiceEnd`, `Builtin`, `ClearWhich`) puts `#[default] #[serde(other)]` on its default variant. `#[serde(other)]` catches only unknown **strings**, so every `place` field (`TimerDef.place`, `PreService.place`, `MessageTemplate.place`) also has `#[serde(deserialize_with = "crate::lenient::or_default")]`, which turns any other JSON (an object, a number) into the default (Plan decision 18). Optional fields that a bad value could fail (`ServiceItem.advance`, `Service.starts_at`) use a never-failing `deserialize_with` (`advance::lenient_advance`, `timers::lenient_time`). **No change to `store::SUPPORTED_FORMAT`.** No `unwrap`/`expect` on stored or user data.
- **Time:** `chrono` only. Instants are `DateTime<Utc>`. Time-of-day calculations are generic over `Tz: chrono::TimeZone`. Part A logic never calls `Utc::now()` or `Local::now()`; only `SystemClock` does. DST rule: a local time inside the spring-forward gap uses the first valid minute after it, and an ambiguous (fall-back) time uses the earlier instant.
- **Strings:** every user-facing core string is a `pub const` (or a `pub fn` for formatted ones) in `src/strings/automation.rs`, copied verbatim from the spec with curly quotes `“ ”`, the apostrophe `'` and the em dash `—`. Strings marked *(plan copy)* below are not fixed by the spec. They are written in the same plain voice, and Jake can reword them.
- **Wording rules (FEAT-1 §7):** the user words are "overlay", "theme" and "screen view", never "prop", "look", "layer" or "z-order". Clock and time labels are 12-hour with AM/PM by default (spec decision 32).
- **Part B** is blocked until `native-foundation` has Plans 3 and 4 merged and FEAT-1 Part B (overlay layer, `clear_epoch`, `Data::after_load`) is in. Tasks that use FEAT-3, FEAT-6 or FEAT-7 hooks name them in a **Needs** line. If one of those is not merged yet, the task does the rest and keeps that hook's sub-step open in the checklist.

### Plan decisions (made here; Jake can change any)

1. **`OverlayPlace` choice list instead of storing `Placement`.** `TimerDef.place` and `PreService.place` store `OverlayPlace { TopRight (default), TopLeft, BottomRight, BottomLeft, CenterLarge }`, persisted as a snake_case string (`"top_right"`). The name is FEAT-1 §4.5's name for this list. FEAT-1 §4.5 maps it to `Placement` when a slot is built (corners → same anchor, width 25, margin 4; `CenterLarge` → Center, width 80). This matches the five choices in spec §3.2, and Part A does not depend on FEAT-1 code. `OverlayPlace::placement()` is added in Task B3. **This deviates on purpose from FEAT-2 spec §4.2**, which types the field as a `Placement` object; see the deviations table under Spec coverage.
2. **Snapshots are quantised; `text` only formats.** `TimerSnapshot.value_ms` is always a whole number of seconds × 1000: countdown time left rounds **up**, and count-up and overrun round **down**. `timers::text(value_ms)` truncates the absolute value to seconds. So every caller of `text` gets the spec's rounding without knowing the kind.
3. **`seen_clear` is captured on a fresh start (from Stopped or Finished) and on restart, not when resuming from Paused.** A clear during a pause still blocks the at-zero macro's show steps, so the clear wins.
4. **Auto-advance waits for the commit.** An `AutoNext` is not due while `waiting_for` is set (spec §5.6, "slow preparation doesn't shorten a slide"). Task B6 makes sure a closed room still lets slides turn (Review Focus 2).
5. **One `AutoState` per engine** bundles `AutoRun` and the paused item (the spec has two maps, `auto` and `auto_paused_item`). It is memory only, so nothing is stored differently.
6. **Message fill errors in a macro** (a template gained a field after the macro was saved) are a run-time step failure: "Ran “Walk-in”, but step 3 didn't work: Fill in “Plate”." This reuses spec copy, so no new wording is needed.
7. **Time labels in messages** use 12-hour form: "The service start time (10:30 AM) has passed."
8. **Built-in timer flags:** Sermon has `on_stage: true` (spec §3.2). Every other built-in flag is false. Captions: Service start and Pre-service use "Service starts in"; Sermon and Stopwatch have none.
9. **`MacroStep::ScreenView.screen` is a `String`** (FEAT-7's `OutputRole` name, e.g. `"room"`) until FEAT-7 lands. Task B5 switches it to `OutputRole` if that type serialises as the same snake_case string. Otherwise it maps the name.
10. **`Builtin` is `{ Unknown (default, serde other), PreService }`**, and `MacroColor` is `{ Amber (default), Red, Orange, Green, Teal, Blue, Purple, Gray }`.
11. **Fixed ids** are `Uuid::from_u128` constants: timers `0x52500000_0000_4000_8000_0000000000{01,02,03}` (Service start, Sermon, Stopwatch) and `…04` (Pre-service); messages `…11`, `…12`, `…13` (Nursery, Vehicle, Custom); the pre-service macro is `…21`.
12. **`seed(&mut Automation) -> bool`**, not the spec's `seed(&mut Data) -> bool` (§4.2), because Part A can't name `Data`. B1 calls it from `Data::after_load` as `automation::seed(&mut self.automation)`; the return value means the same thing.
13. **`apply_batch` takes `&[BatchStep]`** with `BatchStep::Live { step: LiveStep, show: bool }` and `BatchStep::Runtime(usize)`, a runtime callback, and returns `BatchOutcome { outcome, failed, skipped_shows }`. The spec (§5.2) has `steps: &[LiveStep]` and `-> Outcome`. Runtime steps need their own checkpoints inside the same batch, and the toast needs the failed and skipped step numbers. The show flag lives in the step, not in a parallel slice.
14. **`AutomationRuntime::on_adopt(item_id, &map)`** is the spec's `Automation::on_adopt` (§4.3, §5.6). In this plan `Automation` is the persisted root (A11), and the engines' `AutoState`s live in the runtime (A13). FEAT-4's plan names the same path.
15. **A Stopped countdown-to-a-time keeps the ticker awake while its readout is visible.** Spec §4.4 shows the time left to the target while Stopped, so that value changes every second. `AutomationRuntime::is_idle` counts a Stopped `UntilTime` timer as busy when it is shown in a frame, is `on_remote`, or the Automation dialog is open. Otherwise (for example, the built-in Service start timer with nothing showing it) the ticker sleeps and the value is recomputed on the next wake.
16. **The strip's two-row limit applies at window heights of 800 px or less** (`strip::TWO_ROW_MAX_HEIGHT`). The spec fixes the rule at 768 px and says taller windows show all four rows; 800 covers 768 plus title-bar rounding.
17. **`lenient` and `timers` are crate-root modules declared in `src/lib.rs` from Part A on** (not `#[path]` children of `automation`), so FEAT-3 Part A can import `crate::lenient` and `crate::timers` directly. FEAT-3's plan records the same decision.
18. **`place` fields load leniently.** `crate::lenient::or_default` (A1) reads a `serde_json::Value` and falls back to `T::default()` on any error, so a `Placement` object written by a newer version, or any other non-string, loads as `TopRight`/`Bottom` instead of failing `Store::load`.

## Review Focus

1. **The computer clock jumps backwards** (NTP fix, time zone change, or a laptop waking with a corrected clock) while a timer runs, so `now < since`. Expected: elapsed time is clamped at 0. A countdown never shows more than its full length and never panics on a negative `TimeDelta`. Test `clock_going_backwards_never_exceeds_full_length` in Task A5.
2. **Auto-advance with the room window closed.** If nothing ever commits, `waiting_for` never clears. Expected: slides still turn on the operator preview every N seconds, because with no room connected the App treats a change as committed straight away. Test `auto_advances_with_room_closed` in Task B6.
3. **Typed values that contain braces.** The volunteer types `{Plate}` or `}}` into a field. Expected: the value is put in literally and never parsed again as a field. Test `values_are_not_reparsed` in Task A7.
4. **Long messages in non-English letters.** 200 letters of "é", or emoji. Expected: the 200 limit counts letters (`chars`), not bytes, so 200 "é" is accepted. Test `fill_counts_letters_not_bytes` in Task A7.
5. **A timer deleted while it runs or is at zero.** Expected: its leftover runtime entry is ignored. It is left out of the view, fires no at-zero and does not keep the ticker awake. Tests `view_skips_runs_of_deleted_timers` and `zero_crossings_ignore_deleted_timers` in Task A13.

---

# PART A — Core, buildable now

### Task A1: `Lenient<T>`, the module root and the strings file

**Files:**
- Create: `src/lenient.rs`, `src/automation/mod.rs`, `src/strings/automation.rs`
- Modify: `src/lib.rs` (two lines: `pub mod lenient;` and `pub mod automation;`)

**Interfaces:**
- Produces: `#[derive(Clone, Debug, PartialEq)] pub enum Lenient<T> { Known(T), Unknown(serde_json::Value) }` with `pub fn known(&self) -> Option<&T>`, `pub fn known_mut(&mut self) -> Option<&mut T>`, `impl<T: Default> Default` (→ `Known(T::default())`), `impl<T> From<T>`, `impl<T: Serialize> Serialize` (Known → `T`; Unknown → the raw value unchanged) and `impl<'de, T: DeserializeOwned> Deserialize<'de>` (reads a `serde_json::Value`; `serde_json::from_value::<T>(v.clone())` → `Known`, otherwise `Unknown(v)`; never fails).
- Also `pub fn or_default<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error>`: reads a `serde_json::Value` and returns `serde_json::from_value::<T>(v)` or `T::default()` on any error; it fails only if the input is not JSON at all (Plan decision 18).
- `src/automation/mod.rs` in this task: `#[path = "../strings/automation.rs"] pub mod strings;` and a doc comment that says the module is FEAT-2's and Task B1 turns `strings` into `pub use crate::strings::automation as strings;`.
- `src/strings/automation.rs`: a header comment saying this is FEAT-2's string file (FEAT-4 §7 convention), plus `pub const STALE_TRY_AGAIN: &str = "The screen changed — try again.";` and `pub const CLEARED_TRY_AGAIN: &str = "The screen was cleared — try again.";`. Later tasks add their own strings.

- [ ] **Step 1: Write the failing tests** in `src/lenient.rs`, and in the same step add `pub mod lenient;` and `pub mod automation;` to `src/lib.rs` and create `src/automation/mod.rs` (doc comment and the `strings` line only) and `src/strings/automation.rs` (header comment only). The tests use a test-only `#[serde(tag = "type", rename_all = "snake_case")] enum Shape { Dot, Line { len: u32 } }`:

```rust
#[test]
fn known_value_round_trips() {
    let l: Lenient<Shape> = serde_json::from_str(r#"{"type":"line","len":3}"#).unwrap();
    assert_eq!(l, Lenient::Known(Shape::Line { len: 3 }));
    assert_eq!(serde_json::to_string(&l).unwrap(), r#"{"type":"line","len":3}"#);
}

#[test]
fn unknown_value_is_kept_and_written_back_unchanged() {
    let src = r#"{"type":"spiral","turns":7,"extra":[1,2]}"#;
    let l: Lenient<Shape> = serde_json::from_str(src).unwrap();
    assert!(matches!(l, Lenient::Unknown(_)));
    assert_eq!(serde_json::to_value(&l).unwrap(), serde_json::from_str::<serde_json::Value>(src).unwrap());
}

#[test]
fn lenient_never_fails_the_containing_struct() {
    #[derive(Deserialize)] struct Holder { items: Vec<Lenient<Shape>> }
    let h: Holder = serde_json::from_str(r#"{"items":[{"type":"dot"},5,"x",{"type":"line"}]}"#).unwrap();
    assert_eq!(h.items[0], Lenient::Known(Shape::Dot));
    assert!(h.items[1..].iter().all(|i| matches!(i, Lenient::Unknown(_))));
}

#[test]
fn or_default_turns_any_bad_value_into_default() {
    #[derive(Deserialize, Default, Debug, PartialEq)] #[serde(rename_all = "snake_case")]
    enum Place { #[default] #[serde(other)] TopRight, TopLeft }
    #[derive(Deserialize)] struct Holder { #[serde(default, deserialize_with = "or_default")] place: Place }
    let p = |s: &str| serde_json::from_str::<Holder>(s).unwrap().place;
    assert_eq!(p(r#"{"place":"top_left"}"#), Place::TopLeft);
    assert_eq!(p(r#"{"place":"middle"}"#), Place::TopRight);
    assert_eq!(p(r#"{"place":{"anchor":"top_left","width":25,"margin":4}}"#), Place::TopRight);
    assert_eq!(p(r#"{"place":7}"#), Place::TopRight);
    assert_eq!(p(r#"{}"#), Place::TopRight);
}
```

- [ ] **Step 2: Run** `& $FC test --release --lib lenient::`. Expected: FAIL to compile, `cannot find type Lenient` / `cannot find function or_default`.
- [ ] **Step 3: Implement** `Lenient<T>`, `or_default`, the `STALE_TRY_AGAIN`/`CLEARED_TRY_AGAIN` strings and the module doc comment.
- [ ] **Step 4: Run** `& $FC test --release --lib lenient::`. Expected: `running 4 tests`, 4 passed. Also run `& $FC test --release --lib`: every existing test still passes.
- [ ] **Step 5: Commit** `feat(automation): Lenient<T> for data from newer versions; module root`

### Task A2: Timer definitions, `TimeOfDay` and the clock

**Files:**
- Create: `src/timers.rs`
- Modify: `src/lib.rs` (add `pub mod timers;`)

**Interfaces:**
- Consumes: `Lenient<T>` (A1).
- Produces (all `Serialize, Deserialize, Clone, Debug, PartialEq`):
  - `#[derive(Copy, Eq, Ord, PartialOrd, Default)] pub struct TimeOfDay { pub hour: u8, pub minute: u8 }`. `pub fn new(hour: u8, minute: u8) -> Self` clamps to 23 and 59. `pub fn parse(s: &str) -> Option<Self>`: trims the text, then needs 1–2 digits, `:`, and exactly 2 digits; values out of range are clamped. `pub fn label(&self) -> String` is 12-hour (`"10:30 AM"`, `"12:05 PM"`, `"12:00 AM"`). It serialises as `"HH:MM"` (zero-padded). Deserialising a string that `parse` rejects is an error, which makes a containing `Lenient` `Unknown`.
  - `pub fn lenient_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<TimeOfDay>, D::Error>` never fails: null, a missing value, a non-string or unparsable text → `None`. It is used later as `#[serde(default, deserialize_with = "lenient_time")]`.
  - `#[serde(tag = "type", rename_all = "snake_case")] pub enum TimerKind { Duration { secs: u32 }, UntilTime { target: TimeTarget }, CountUp }`, `Default` = `CountUp`; `pub fn is_countdown(&self) -> bool`.
  - `#[serde(tag = "type", rename_all = "snake_case")] pub enum TimeTarget { ServiceStart, Fixed { at: TimeOfDay } }`.
  - `#[serde(tag = "type", rename_all = "snake_case")] pub enum AtZero { #[default] Nothing, HideFromRoom, RunMacro { macro_id: Uuid } }`.
  - Unit enums (`rename_all = "snake_case"`, `#[default] #[serde(other)]` on the first variant): `Overrun { Stop, CountUp }`, `OverlayPlace { TopRight, TopLeft, BottomRight, BottomLeft, CenterLarge }` (Plan decision 1), `Builtin { Unknown, PreService }`.
  - `#[serde(default)] #[derive(Default)] pub struct TimerDef { pub id: Uuid, pub name: String, pub kind: Lenient<TimerKind>, pub overrun: Overrun, pub at_zero: Lenient<AtZero>, #[serde(deserialize_with = "crate::lenient::or_default")] pub place: OverlayPlace, pub caption: String, pub on_stage: bool, pub on_remote: bool, pub builtin: Option<Builtin> }`.
  - `pub const SERVICE_START_ID`, `SERMON_ID`, `STOPWATCH_ID`, `PRE_SERVICE_TIMER_ID: Uuid` (Plan decision 11).
  - `pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; }`, `pub struct SystemClock;` (`Utc::now()`), `pub struct FakeClock(Mutex<DateTime<Utc>>)` with `new(at)`, `set(at)` and `advance_ms(ms: i64)`. It is plain `pub`, not `cfg(test)`, because Part B app tests use it.

- [ ] **Step 1: Write the failing tests** in `src/timers.rs`, and add `pub mod timers;` to `src/lib.rs` in this step:
  - `time_of_day_serializes_hh_mm`: `TimeOfDay::new(9, 5)` → `"\"09:05\""`. `parse("9:05")` and `parse(" 09:05 ")` both give `9:05`. `parse("9:5")`, `parse("9")`, `parse("")` and `parse("abc")` give `None`. `label()` is `"9:05 AM"`, `TimeOfDay::new(0, 0).label()` is `"12:00 AM"` and `TimeOfDay::new(12, 5).label()` is `"12:05 PM"`.
  - `bad_time_of_day_clamps_or_drops_field`: `parse("25:70") == Some(TimeOfDay::new(23, 59))`. For a `Holder { #[serde(default, deserialize_with = "lenient_time")] at: Option<TimeOfDay> }`, the inputs `{"at":"soon"}`, `{"at":null}`, `{"at":7}` and `{}` all give `None`. `serde_json::from_str::<Lenient<TimerKind>>(r#"{"type":"until_time","target":{"type":"fixed","at":"soon"}}"#)` is `Unknown`.
  - `unknown_timer_kind_loads_and_round_trips`: `{"type":"lap_timer","laps":3}` → `Unknown`, and it serialises back to the same `Value`.
  - `unknown_unit_enum_value_loads_as_default`: a `TimerDef` from `{"overrun":"explode","place":"middle","builtin":"robot"}` has `Overrun::Stop`, `OverlayPlace::TopRight` and `Some(Builtin::Unknown)`.
  - `place_object_from_newer_version_loads_as_default`: a `TimerDef` from `{"name":"X","place":{"anchor":"bottom_left","width":25,"margin":4}}` loads with `place == OverlayPlace::TopRight` and `name == "X"`; `{"place":"center_large"}` → `CenterLarge`.
  - `timer_def_missing_fields_default`: `{}` gives `TimerDef::default()`, with `kind == Lenient::Known(TimerKind::CountUp)` and `at_zero == Lenient::Known(AtZero::Nothing)`.
  - `timer_kind_json_shape`: `Duration { secs: 1800 }` → `{"type":"duration","secs":1800}`; `UntilTime { target: ServiceStart }` → `{"type":"until_time","target":{"type":"service_start"}}`; `Fixed { at: 10:30 }` → `{"type":"fixed","at":"10:30"}`; `AtZero::RunMacro` → `{"type":"run_macro","macro_id":"…"}`.
  - `fake_clock_advances`: `FakeClock::new(t0)`, then `advance_ms(1500)` → `now() == t0 + 1.5 s`.
- [ ] **Step 2: Run** `& $FC test --release --lib timers::`. Expected: FAIL to compile (`cannot find type TimeOfDay`, …).
- [ ] **Step 3: Implement** the types above. `TimeOfDay` gets a hand-written `Serialize`/`Deserialize` (string form).
- [ ] **Step 4: Run** `& $FC test --release --lib timers::`. Expected: `running 8 tests`, 8 passed.
- [ ] **Step 5: Commit** `feat(timers): timer definitions, TimeOfDay and Clock with lenient serde`

### Task A3: Readout types and the shared formatter

**Files:**
- Modify: `src/timers.rs` (a new file from A2)

**Interfaces:**
- Produces (the FEAT-3 contract, spec §4.4; names are final):
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)] pub enum TimerState { Stopped, Running, Paused, Finished }`
  - `#[derive(Clone, Debug, PartialEq, Serialize)] pub struct TimerSnapshot { pub id: Uuid, pub name: String, pub kind: TimerKind, pub state: TimerState, pub value_ms: i64, pub overrun: bool, pub on_stage: bool, pub on_remote: bool }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Serialize)] pub struct AutoReadout { pub next_in_secs: u32, pub paused: bool }`
  - `#[derive(Clone, Debug, PartialEq, Default, Serialize)] pub struct TimersView { pub timers: Vec<TimerSnapshot>, pub auto: Option<AutoReadout> }`
  - `pub fn text(value_ms: i64) -> String`: `s = |value_ms| / 1000`, truncated. Under an hour the form is `m:ss`, from an hour `h:mm:ss`. A `+` prefix appears only when `value_ms < 0` and `s > 0`.
  - `pub fn display_ms(raw_ms: i64, countdown: bool) -> i64` (Plan decision 2): a countdown with `raw_ms > 0` rounds up to the next 1000. Anything else rounds the absolute value down to 1000 and keeps the sign.

- [ ] **Step 1: Write the failing test** `timer_text_shared_formatter`:

```rust
for (ms, s) in [(0, "0:00"), (59_000, "0:59"), (60_000, "1:00"), (3_599_000, "59:59"), (3_600_000, "1:00:00"),
                (-72_000, "+1:12"), (-3_900_000, "+1:05:00"), (-400, "0:00")] {
    assert_eq!(text(ms), s, "{ms}");
}
assert_eq!(display_ms(1, true), 1000);
assert_eq!(display_ms(1500, true), 2000);
assert_eq!(display_ms(2000, true), 2000);
assert_eq!(display_ms(0, true), 0);
assert_eq!(display_ms(1999, false), 1000);
assert_eq!(display_ms(-1999, true), -1000);
```

- [ ] **Step 2: Run** `& $FC test --release --lib timers::tests::timer_text`. Expected: FAIL to compile (`cannot find function text`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 1 test`, 1 passed.
- [ ] **Step 5: Commit** `feat(timers): TimerSnapshot/TimersView and the shared text formatter`

### Task A4: Countdown-to-a-time targets, with DST

**Files:**
- Modify: `src/timers.rs`, `src/strings/automation.rs`

**Interfaces:**
- Produces:
  - `pub fn resolve_local<Tz: TimeZone>(tz: &Tz, date: NaiveDate, at: TimeOfDay) -> DateTime<Utc>`. `tz.from_local_datetime(..)` gives `Single` (use it), `Ambiguous` (use `.earliest()`), or `None`, a gap. For a gap, step the naive time forward one minute at a time (at most 180 steps) until it resolves.
  - `pub fn until_target<Tz: TimeZone>(target: &TimeTarget, starts_at: Option<TimeOfDay>, now: DateTime<Utc>, tz: &Tz) -> Result<DateTime<Utc>, String>`. "Today" is `now.with_timezone(tz).date_naive()`. The rules (spec §5.1):
    - `ServiceStart` with no start time → `NO_START_TIME`.
    - `ServiceStart` at a time `<= now` → `service_start_passed(label)`.
    - `Fixed{at}` later today → that time.
    - `Fixed{at}` passed under 60 min ago → `time_passed(label)`.
    - `Fixed{at}` passed 60 min or more ago → tomorrow at `at`.
- Strings: `NO_START_TIME = "Set a start time for this service first (Services → Starts at)."`, `pub fn service_start_passed(at: &str) -> String` → "The service start time ({at}) has passed.", `pub fn time_passed(at: &str) -> String` → "That time ({at}) has passed.".
- Test-only `struct Chicago2026` (`#[cfg(test)] mod test_zone` in `src/timers.rs`) implements `chrono::TimeZone` with `Offset = FixedOffset`. CDT (−5 h) applies from `2026-03-08T08:00Z` up to (not including) `2026-11-01T07:00Z`; CST (−6 h) otherwise. `offset_from_local_datetime` tries both offsets and keeps those that map back to themselves: 0 → `None`, 1 → `Single`, 2 → `Ambiguous(cdt, cst)`. The date methods use local midnight.

- [ ] **Step 1: Write the failing tests** (`chi = Chicago2026`; `utc(y,m,d,h,mi)` = `Utc.with_ymd_and_hms(..).unwrap()`):
  - `service_start_without_time_errors`: `until_target(&ServiceStart, None, now, &chi) == Err(NO_START_TIME.into())`.
  - `start_after_target_runs_at_zero_or_refuses`: `now = utc(2026,6,7,15,45)` (10:45 CDT, a Sunday).
    - `ServiceStart` with 10:30 → `Err("The service start time (10:30 AM) has passed.")`.
    - `Fixed 10:15` → `Err("That time (10:15 AM) has passed.")`.
    - `Fixed 7:45` → `Ok(utc(2026,6,8,12,45))`, tomorrow.
    - `Fixed 11:00` → `Ok(utc(2026,6,7,16,0))`.
  - `until_time_across_dst_change`:
    - March: `now = utc(2026,3,8,7,0)` (1:00 CST). `Fixed 2:30` (in the gap) → `utc(2026,3,8,8,0)` (3:00 CDT). `Fixed 10:30` → `utc(2026,3,8,15,30)`.
    - November: `now = utc(2026,11,1,5,30)` (0:30 CDT). `Fixed 1:30` (ambiguous) → `utc(2026,11,1,6,30)` (the earlier, CDT). `Fixed 10:30` → `utc(2026,11,1,16,30)` (CST).
- [ ] **Step 2: Run** `& $FC test --release --lib timers::`. Expected: the new tests fail to compile.
- [ ] **Step 3: Implement** `resolve_local`, `until_target`, the strings and the test zone.
- [ ] **Step 4: Run** the same command. Expected: `running 12 tests`, all passed.
- [ ] **Step 5: Commit** `feat(timers): countdown-to-time targets with DST gap/overlap rules`

### Task A5: Timer state machine and snapshots

**Files:**
- Modify: `src/timers.rs`, `src/strings/automation.rs`

**Interfaces:**
- Consumes: `TimerDef`, `until_target`, `display_ms`, `TimerSnapshot`.
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Default)] pub enum RunState { #[default] Stopped, Running { since: DateTime<Utc>, carried_ms: u64 }, RunningTo { target: DateTime<Utc> }, Paused { elapsed_ms: u64 }, Finished }`
  - `#[derive(Clone, Debug, PartialEq, Default)] pub struct TimerRun { pub state: RunState, pub run: u64, pub zero_fired: bool, pub length_for_now: Option<u32>, pub seen_clear: u64 }`
  - `pub struct StartCtx<'a, Tz: TimeZone> { pub now: DateTime<Utc>, pub tz: &'a Tz, pub starts_at: Option<TimeOfDay>, pub clear_epoch: u64 }`
  - `impl TimerRun`:
    - `pub fn start<Tz: TimeZone>(&mut self, def: &TimerDef, ctx: &StartCtx<Tz>) -> Result<(), String>`. Unknown kind → `TIMER_NEEDS_NEWER`.
      - From Stopped or Finished: `Duration`/`CountUp` → `Running{since: now, carried_ms: 0}`, and `UntilTime` → `RunningTo{target}`. It sets `seen_clear = ctx.clear_epoch`. From Finished it also does `run += 1` and `zero_fired = false`.
      - From Paused: `Running{since: now, carried_ms: elapsed}`.
      - From Running/RunningTo: no-op `Ok`.
      - A refused target leaves the state unchanged.
    - `pub fn pause(&mut self, now: DateTime<Utc>) -> Result<(), String>`: Running → `Paused{elapsed}`; RunningTo → `CANT_PAUSE_TO_TIME`; anything else is a no-op `Ok`.
    - `pub fn reset(&mut self)`: → Stopped, `run += 1`, `zero_fired = false`, `length_for_now = None`.
    - `pub fn restart<Tz: TimeZone>(&mut self, def, ctx) -> Result<(), String>`: → `Running` from 0 or `RunningTo` with a new target, `run += 1`, `zero_fired = false`, `seen_clear` captured. On `Err` nothing changes.
    - `pub fn elapsed_ms(&self, now) -> u64`: Running → `carried_ms + max(0, now − since)`; Paused → `elapsed_ms`; otherwise 0.
    - `pub fn snapshot<Tz: TimeZone>(&self, def: &TimerDef, now, tz: &Tz, starts_at: Option<TimeOfDay>) -> Option<TimerSnapshot>`: `None` for an unknown kind. The raw value is:
      - `Duration`: `length − elapsed`, where length is `length_for_now.unwrap_or(secs) × 1000`.
      - `CountUp`: `elapsed`.
      - `UntilTime`, while RunningTo: `target − now`. While Stopped: `until_target(..)` − now, or 0 if that is refused.
      - Finished: 0.
      
      `Overrun::Stop` clamps a negative raw value to 0. `overrun = raw < 0` after that. `value_ms = display_ms(raw, kind.is_countdown())`. The state maps Running/RunningTo → `Running`.
- Strings: `TIMER_NEEDS_NEWER = "This timer needs a newer RustyPresenter."`, `CANT_PAUSE_TO_TIME = "A countdown to a time can't be paused."`.

- [ ] **Step 1: Write the failing tests.** Helpers: `dur(secs, overrun) -> TimerDef`, `ctx(now) -> StartCtx<'static, Utc>` with `tz: &Utc`, `at(ms) = t0 + TimeDelta::milliseconds(ms)`.

```rust
#[test]
fn countdown_rounds_up_and_stops_at_zero() {
    let def = dur(30, Overrun::Stop);
    let mut r = TimerRun::default();
    r.start(&def, &ctx(at(0))).unwrap();
    let v = |r: &TimerRun, ms| text(r.snapshot(&def, at(ms), &Utc, None).unwrap().value_ms);
    assert_eq!(v(&r, 500), "0:30");
    assert_eq!(v(&r, 29_001), "0:01");
    assert_eq!(v(&r, 31_000), "0:00"); // Stop clamps even before the ticker marks it Finished
}
```

  Also:
  - `countdown_overrun_counts_up_with_plus`: `dur(30, CountUp)` at 31 500 ms → `value_ms == -1000`, `text == "+0:01"`, `overrun`, state `Running`.
  - `count_up_formats_hours`: `CountUp` at 3 725 400 ms → `"1:02:05"`.
  - `pause_and_resume_carry_elapsed`: `dur(60)`. Start at 0, pause at 10 s, start again at 100 s, look at 105 s → `value_ms == 45_000`.
  - `stopped_shows_start_value`: a Stopped `dur(1800)` → `"30:00"`; `CountUp` → `"0:00"`; `UntilTime Fixed` 11:00 at 10:45 (`Chicago2026`) → `"15:00"`.
  - `until_time_target_fixed_at_start`: after start, `state == RunningTo{target}` with the A4 target.
  - `editing_starts_at_does_not_move_running_timer`: start `UntilTime{ServiceStart}` with 11:00 at 10:45, then take the snapshot with `starts_at = 11:30` → still `"15:00"`.
  - `until_time_overrun_past_60_min_keeps_counting_up`: `UntilTime`, `Overrun::CountUp`, 65 min after the target → `"+1:05:00"`.
  - `until_time_refuses_pause`: `pause` → `Err(CANT_PAUSE_TO_TIME)`, state unchanged.
  - `unknown_timer_kind_loads_and_cannot_start`: `kind = Lenient::Unknown(..)` → `start` is `Err(TIMER_NEEDS_NEWER)` and `snapshot` is `None`.
  - `restart_bumps_run_and_captures_seen_clear`: `clear_epoch: 7` → `run == 1`, `seen_clear == 7`, back at full length.
  - `resume_keeps_seen_clear`: start with epoch 1, pause, start with epoch 5 → `seen_clear == 1`.
  - `refused_restart_keeps_state`: `def = UntilTime{ServiceStart}`. Start it with `starts_at: Some(11:00)` at 10:45 (`Chicago2026`) → `RunningTo{target: 11:00}` and `run == 0`. Then `restart` with `starts_at: None` → `Err(NO_START_TIME)`, and the state is still `RunningTo{target: 11:00}` (the same target), `run == 0` and `seen_clear` unchanged.
  - `clock_going_backwards_never_exceeds_full_length` (Review Focus 1): start `dur(60)` at `at(10_000)`, take the snapshot at `at(0)` → `value_ms == 60_000`, and there is no panic.
- [ ] **Step 2: Run** `& $FC test --release --lib timers::`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 26 tests`, all passed.
- [ ] **Step 5: Commit** `feat(timers): timer state machine and quantised snapshots`

### Task A6: "Set for now" and firing at zero

**Files:**
- Modify: `src/timers.rs`, `src/strings/automation.rs`

**Interfaces:**
- Produces:
  - `pub enum SetNow { Secs(u32), At(TimeOfDay) }`
  - `TimerRun::set_now<Tz: TimeZone>(&mut self, def: &TimerDef, change: SetNow, ctx: &StartCtx<Tz>) -> Result<(), String>`:
    - `Duration` + `Secs(s)`: `s` must be in `1..=86_399` (else `DURATION_RANGE`). Sets `length_for_now = Some(s)` and leaves the state alone.
    - `UntilTime` + `At(t)` while RunningTo: `target = until_target(&Fixed{at: t}, ..)`. While Stopped it is refused with `SET_NOW_WRONG_KIND`, because the target is fixed only at start.
    - Any other pair → `SET_NOW_WRONG_KIND`. The definition is never changed (it is `&TimerDef`).
  - `TimerRun::check_zero(&mut self, def: &TimerDef, now) -> bool`. For a countdown kind in Running/RunningTo whose raw value is `<= 0` and with `!zero_fired`: set `zero_fired = true`. With `Overrun::Stop`, also set `state = Finished`. Return `true` once per run.
- Strings: `DURATION_RANGE = "Choose a length from 1 second to 23:59:59."` *(plan copy)*, `SET_NOW_WRONG_KIND = "This timer can't be set that way."` *(plan copy)*.

- [ ] **Step 1: Write the failing tests:**
  - `set_for_now_does_not_change_definition`: start Running `dur(1800)`, then at 60 s `set_now(Secs(1500))` → snapshot `"24:00"`. `def` is unchanged. `reset()` → Stopped snapshot `"30:00"` and `length_for_now == None`.
  - `reset_clears_zero_fired_and_set_for_now`: fire zero, then `reset` → `zero_fired == false`, `run` bumped, `length_for_now == None`.
  - `set_now_moves_until_time_target_only_while_running`.
  - `set_now_rejects_out_of_range_and_wrong_kind`: `Secs(0)`, `Secs(86_400)` → `DURATION_RANGE`; `CountUp` + `Secs(5)` → `SET_NOW_WRONG_KIND`.
  - `at_zero_fires_once`: `dur(10, Stop)`. At 9 s → false; at 10 s → true and `Finished`; at 11 s → false; after `restart`, at 10 s past the restart → true again.
  - `count_up_overrun_fires_once_and_keeps_running`: `dur(10, CountUp)` at 10 s → true and still `Running`; at 12 s → false.
  - `paused_countdown_never_fires`.
- [ ] **Step 2: Run** `& $FC test --release --lib timers::`. Expected: FAIL to compile (`cannot find type SetNow`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 33 tests`, all passed.
- [ ] **Step 5: Commit** `feat(timers): set-for-now and once-per-run zero detection`

### Task A7: Message templates — parse, fill and built-ins

**Files:**
- Create: `src/automation/messages.rs`
- Modify: `src/automation/mod.rs` (`pub mod messages;`), `src/strings/automation.rs`

**Interfaces:**
- Produces:
  - `#[serde(rename_all = "snake_case")] pub enum MessagePlace { #[default] #[serde(other)] Bottom, Top }`
  - `#[serde(default)] #[derive(Default)] pub struct MessageTemplate { pub id: Uuid, pub name: String, pub text: String, #[serde(deserialize_with = "crate::lenient::or_default")] pub place: MessagePlace, pub hide_after_secs: Option<u32>, pub remote: bool }`
  - `pub const NURSERY_ID`, `VEHICLE_ID`, `CUSTOM_ID: Uuid`. `pub fn builtin() -> Vec<MessageTemplate>` returns, in order: Nursery "Parent of child #{Number}, please come to the nursery.", Vehicle "Vehicle {Plate}: your lights are on.", Custom "{Message}". All are at the bottom, with no auto-hide and `remote: false`.
  - `pub const HIDE_AFTER_CHOICES: [Option<u32>; 6] = [None, Some(15), Some(30), Some(60), Some(120), Some(300)]`.
  - `pub struct Parsed { pub fields: Vec<String>, pub warning: Option<&'static str> }` and `pub fn parse(text: &str) -> Parsed`. Scan left to right:
    - `{{` → literal `{`, and `}}` → literal `}`.
    - `{` followed by 1–30 characters with no `{` or `}`, then `}` → a field. Fields are listed in order of first appearance, without duplicates.
    - Any other `{` → literal `{` and `warning = Some(UNMATCHED_BRACE)`.
    - A lone `}` → literal, with no warning.
  - `pub fn fill(text: &str, values: &BTreeMap<String, String>) -> Result<String, String>`. Each field's value is trimmed, and an empty value → `fill_in(field)`. `\r\n`, `\n` and `\r` in the values **and** the template become a space. Values are inserted as-is and are never parsed again. More than 200 `chars` → `MESSAGE_TOO_LONG`.
  - `pub fn preview(text: &str, values: &BTreeMap<String, String>) -> String`: the same substitution, but missing values become empty strings, with no errors and no length check (for the Show message preview line).
- Strings: `fill_in(field: &str) -> String` → "Fill in “{field}”.", `MESSAGE_TOO_LONG = "That message is too long for the screen (200 letters at most)."`, `UNMATCHED_BRACE = "This message has a { without a matching }."`, `MESSAGE_SHOWING = "Message showing"`, `PRACTICE_WARNING = "Practice mode is on — this shows only in your preview. End practice to show it in the room."` (spec §3.3, shown by B10), and the built-in names and texts.

- [ ] **Step 1: Write the failing tests** in `src/automation/messages.rs` (spec §9 names, with these values), and add `pub mod messages;` to `src/automation/mod.rs` in this step:
  - `fields_in_order_without_duplicates`: `parse("{A} and {B} and {A}").fields == ["A", "B"]`; Nursery → `["Number"]`.
  - `double_braces_are_literal`: `parse("{{Number}}").fields` is empty, and `fill` → `"{Number}"`.
  - `stray_brace_is_literal_with_warning`: `parse("50% off {today")` → no fields and `warning == Some(UNMATCHED_BRACE)`; `fill` → `"50% off {today"`. A 31-character name in braces is also stray.
  - `fill_requires_every_field`: Nursery with `{"Number": "  "}` → `Err("Fill in “Number”.")`.
  - `fill_trims_and_flattens_newlines`: Custom with `"  Hello\r\nworld  "` → `"Hello world"`.
  - `fill_rejects_over_200_chars`: 201 × `a` → `Err(MESSAGE_TOO_LONG)`; exactly 200 → `Ok`.
  - `fill_counts_letters_not_bytes` (Review Focus 4): 200 × `é` → `Ok`.
  - `values_are_not_reparsed` (Review Focus 3): Vehicle with `{"Plate": "{Plate}}"}` → `"Vehicle {Plate}}: your lights are on."`.
  - `builtin_templates_have_fixed_ids_and_text`.
  - `unknown_place_loads_as_bottom`: `{"place":"side"}` and `{"place":{"anchor":"top"}}` both load as `Bottom`.
  - `practice_warning_is_exact`.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::messages`. Expected: FAIL to compile (`cannot find function parse`, …).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 11 tests`, all passed.
- [ ] **Step 5: Commit** `feat(automation): message templates — field parsing, filling, built-ins`

### Task A8: Macros — steps, classes and up-front resolution

**Files:**
- Create: `src/automation/macros.rs`
- Modify: `src/automation/mod.rs` (`pub mod macros;`), `src/strings/automation.rs`

**Interfaces:**
- Consumes: `Lenient`, `TimerDef`, `TimerRun`, `until_target`, `MessageTemplate`, `messages::fill`.
- Produces:
  - `MacroStep` exactly as spec §4.2 (`#[serde(tag = "step", rename_all = "snake_case")]`; `ShowItem { target: ItemTarget, #[serde(default)] slide: usize }`, …), except `ScreenView { screen: String, view: Uuid }` (Plan decision 9).
  - `#[serde(tag = "type", rename_all = "snake_case")] pub enum ItemTarget { Number { n: usize }, Titled { title: String } }`.
  - `pub enum ClearWhich { #[default] #[serde(other)] All, Slide, Media, Audio, Overlays }` and `pub enum MacroColor { #[default] #[serde(other)] Amber, Red, Orange, Green, Teal, Blue, Purple, Gray }` (snake_case).
  - `#[serde(default)] #[derive(Default)] pub struct Macro { pub id: Uuid, pub name: String, pub color: MacroColor, pub steps: Vec<Lenient<MacroStep>>, pub remote: bool, pub builtin: Option<Builtin> }`, with:
    - `pub fn is_relative(&self) -> bool`: any ShowItem, Next, Prev, Clear, Logo, ScreenView, **or Unknown** step.
    - `pub fn is_overlay_only(&self) -> bool`: every step is one of TimerShow, TimerHide, MessageShow, MessageHide, Overlay, TimerStart, TimerPause, TimerReset, TimerRestart, StageMessageShow, StageMessageHide, StageLayout or StageLayoutAuto. An Unknown step → false.
  - `pub enum StepClass { Live, Runtime, Action }`.
  - `pub enum PlannedStep { ShowItem { item_id: Uuid, slide: usize }, Next, Prev, Clear(ClearWhich), Logo, Overlay { id: Uuid, on: bool }, ScreenView { screen: String, view: Uuid }, TimerShow { timer: Uuid }, TimerHide { timer: Uuid }, MessageShow { template: Uuid, filled: Result<String, String> }, MessageHide, TimerStart { timer: Uuid, target: Option<DateTime<Utc>> }, TimerPause { timer: Uuid }, TimerReset { timer: Uuid }, TimerRestart { timer: Uuid, target: Option<DateTime<Utc>> }, AutoAdvance { on: bool }, StageMessageShow { text: String }, StageMessageHide, StageLayout { layout: Uuid }, StageLayoutAuto }`, with:
    - `pub fn class(&self) -> StepClass`: Live = ShowItem…MessageHide; Runtime = Timer* and AutoAdvance; Action = Stage*.
    - `pub fn is_show(&self) -> bool`: `Overlay{on: true}`, `TimerShow` and `MessageShow`.
  - `pub struct Planned { pub number: usize /* 1-based */, pub step: PlannedStep }` and `pub struct Resolved { pub name: String, pub relative: bool, pub steps: Vec<Planned> }`.
  - `pub struct ResolveCtx<'a, Tz: TimeZone> { pub timers: &'a [TimerDef], pub runs: &'a HashMap<Uuid, TimerRun>, pub templates: &'a [MessageTemplate], pub overlays: &'a [Uuid], pub stage_layouts: &'a [Uuid], pub screen_views: &'a [Uuid], pub items: &'a [(Uuid, String)] /* current service, in order */, pub now: DateTime<Utc>, pub tz: &'a Tz, pub starts_at: Option<TimeOfDay> }`.
  - `pub fn resolve<Tz: TimeZone>(m: &Macro, ctx: &ResolveCtx<Tz>) -> Result<Resolved, String>`. Steps are checked in order, and the first problem wins:
    - An Unknown step → `cant_run_newer(name, n)`.
    - A missing reference → `cant_run_deleted(name, n, thing)`, where `thing` is one of "timer", "message", "overlay", "stage layout", "screen view" or "item".
    - `Number{n}` → item `n − 1`, and 0 or out of range counts as deleted. `Titled` → the first title equal to it ignoring case and outer spaces (`trim().to_lowercase()`).
    - `TimerStart` computes a target only when the timer's kind is `UntilTime` and its run is Stopped or Finished. `TimerRestart` always computes one for `UntilTime`. A refused target returns that `until_target` error unchanged.
    - `MessageShow` fills the text now and keeps the `Result` (Plan decision 6).
- Strings:
  - `cant_run_deleted(name, n, thing)` → "Couldn't run “{name}”: step {n}'s {thing} was deleted. Edit the macro."
  - `cant_run_newer(name, n)` → "Couldn't run “{name}”: step {n} needs a newer RustyPresenter."
  - `ran(name)` → "Ran “{name}”"
  - `ran_but_step_failed(name, n, reason)` → "Ran “{name}”, but step {n} didn't work: {reason}"
  - `ran_but_skipped_shows(name)` → "Ran “{name}”, but skipped showing things because the screen was cleared."
  - `at_zero_changes_slides(t, m)` → "“{t}” reached zero, but “{m}” changes slides, so it didn't run." (used by B5)
  - `at_zero_skipped_shows(t, m)` → "“{t}” reached zero, but “{m}” didn't show anything because the screen was cleared." (used by B5)

- [ ] **Step 1: Write the failing tests** in `src/automation/macros.rs`, and add `pub mod macros;` to `src/automation/mod.rs` in this step:
  - `unknown_macro_step_loads_and_round_trips`: `{"steps":[{"step":"fade_audio","secs":3}]}` → one `Unknown`, which serialises back the same.
  - `jump_and_refresh_are_not_steps`: `{"step":"jump"}`, `{"step":"refresh"}` and `{"step":"run_macro","macro_id":"…"}` are all `Unknown`.
  - `step_json_shape`: `ShowItem{Number{n:1}, slide:0}` ↔ `{"step":"show_item","target":{"type":"number","n":1},"slide":0}`, and a missing `"slide"` → 0. `Clear{which: Overlays}` ↔ `{"step":"clear","which":"overlays"}`.
  - `relative_and_overlay_only_classes`: one table row per step kind for both predicates. An Unknown step is relative and not overlay-only. AutoAdvance is neither relative nor overlay-only.
  - `item_target_by_number_and_title_case_insensitive`: items `[(a,"Welcome"),(b,"Song"),(c,"welcome")]`. `Titled("  WELCOME ")` → `a`; `Number{2}` → `b`; `Number{0}` and `Number{4}` → `Err("Couldn't run “Walk-in”: step 1's item was deleted. Edit the macro.")`.
  - `deleted_target_runs_nothing`: one case each for timer, message, overlay, stage layout and screen view, with the exact strings. When two steps are bad, the error names the first.
  - `unknown_step_refuses_with_newer_message`: step 4 Unknown → "Couldn't run “Walk-in”: step 4 needs a newer RustyPresenter."
  - `refused_timer_target_runs_nothing`: `TimerStart` on `UntilTime{ServiceStart}` with `starts_at: None` → `Err(NO_START_TIME)`. The same step on a timer already `RunningTo` → `Ok`, with `target: None`.
  - `show_item_step_slide_one_is_first_slide`: the stored `slide: 0` resolves to `PlannedStep::ShowItem{slide: 0, ..}`.
  - `message_step_fill_error_is_kept_for_run_time`: a template `"{Plate}"` with empty values → `filled == Err("Fill in “Plate”.")`, and `resolve` is `Ok`.
  - `planned_step_classes_and_show_flags`.
  - `macro_strings_are_exact`: `ran`, `ran_but_step_failed`, `ran_but_skipped_shows`, `at_zero_changes_slides("Sermon", "Walk-in")` and `at_zero_skipped_shows("Sermon", "Walk-in")` against the texts above.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::macros`. Expected: FAIL to compile (`cannot find type MacroStep`, …).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 12 tests`, all passed.
- [ ] **Step 5: Commit** `feat(automation): macro steps, classes and up-front resolution`

### Task A9: Auto-advance state machine

**Files:**
- Create: `src/automation/advance.rs`
- Modify: `src/automation/mod.rs` (`pub mod advance;`), `src/strings/automation.rs`

**Interfaces:**
- Consumes: `AutoReadout` (A3).
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)] pub struct AutoAdvance { pub secs: u16, #[serde(default = "yes")] pub loop_item: bool }`. `pub fn interval(&self) -> TimeDelta` clamps to 2–600 s. `pub fn validate(secs: u16) -> Result<(), &'static str>` → `ADVANCE_RANGE`. `pub const DEFAULT_SECS: u16 = 8`.
  - `pub fn lenient_advance<'de, D: Deserializer<'de>>(d: D) -> Result<Option<AutoAdvance>, D::Error>`: `crate::lenient::or_default::<_, Option<AutoAdvance>>`, so `{}`, a non-number `secs`, a `secs` above `u16::MAX`, a number or a string → `None`, never an error (spec §4.1). B1 puts it on `ServiceItem.advance` and `LiveItem.advance` as `#[serde(default, deserialize_with = "…::lenient_advance")]`.
  - `pub struct AdvanceInput { pub item_id: Uuid, pub slides: usize, pub advance: Option<AutoAdvance> /* the LiveItem snapshot */, pub cursor_slide: usize, pub slide_layer_up: bool, pub version: u64, pub visual_version: u64 }`. The caller passes `None` when nothing is live.
  - `#[derive(Clone, Debug, PartialEq)] pub struct AutoRun { pub item_id: Uuid, pub slide: usize, pub armed_visual: u64, pub due: Option<DateTime<Utc>>, pub waiting_for: Option<u64> }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct AutoNextReq { pub item_id: Uuid, pub from_slide: usize, pub armed_visual: u64 }`
  - `pub enum AutoNextStep { Goto(usize), StopOnLast, Ignore }`
  - `#[derive(Default)] pub struct AutoState { pub run: Option<AutoRun>, pub paused_item: Option<Uuid> }` (one per engine, Plan decision 5):
    - `pub fn reconcile(&mut self, live: Option<&AdvanceInput>, now)`:
      - Clear `paused_item` if the live item is a different one.
      - Remove `run` if there is no live item, no `advance`, `slides < 2`, or the slide layer is down.
      - Otherwise arm `{slide: cursor_slide, armed_visual: visual_version, due: Some(now + interval), waiting_for: Some(version)}`, but only if there is no run, or its `item_id` or `armed_visual` differs.
    - `pub fn on_committed(&mut self, committed: u64, live: Option<&AdvanceInput>, now)`: if `waiting_for == Some(w)` and `committed >= w`, then `due = Some(now + interval)` and `waiting_for = None`.
    - `pub fn due(&self, now) -> Option<AutoNextReq>`: not paused (`paused_item != Some(run.item_id)`), `waiting_for == None`, and `due <= now`.
    - `pub fn stop_on_last(&mut self)`: `due = None`.
    - `pub fn set_on(&mut self, on: bool, live: Option<&AdvanceInput>, now)`: `false` → `paused_item = live item`; `true` → clear it and set `due = Some(now + interval)`, `waiting_for = None`.
    - `pub fn readout(&self, now) -> Option<AutoReadout>`: `None` without a run or with `due == None`. `next_in_secs` = time left rounded up, and never below 0.
    - `pub fn on_adopt(&mut self, item_id: Uuid, map: &[Option<usize>] /* old slide → new */)`: rewrite `run.slide`, or remove the run when the slide is gone. `due` is kept.
  - `pub fn auto_next_step(req: &AutoNextReq, live: Option<&AdvanceInput>) -> AutoNextStep`. It returns `Ignore` unless the same item, `cursor_slide == from_slide`, the slide layer is up and `visual_version == armed_visual` all hold. Otherwise:
    - another slide follows → `Goto(from + 1)`;
    - on the last slide with `loop_item` → `Goto(0)`;
    - on the last slide without it → `StopOnLast`.
- Strings: `ADVANCE_RANGE = "Choose 2 to 600 seconds."` *(plan copy)*, `NEEDS_TWO_SLIDES = "Needs at least two slides."`.

- [ ] **Step 1: Write the failing tests** in `src/automation/advance.rs`, and add `pub mod advance;` to `src/automation/mod.rs` in this step. The helper `live(slide, version, visual)` gives a 3-slide item with `advance = {secs: 8, loop_item: true}`.
  - `auto_advance_json_defaults`: `{"secs":8}` → `loop_item: true`.
  - `bad_advance_drops_field`: for `Holder { #[serde(default, deserialize_with = "lenient_advance")] advance: Option<AutoAdvance>, name: String }`, the inputs `{"advance":{},"name":"A"}`, `{"advance":{"secs":"x"}}`, `{"advance":{"secs":70000}}`, `{"advance":5}`, `{"advance":null}` and `{}` all load with `advance == None` (and `name` kept in the first); `{"advance":{"secs":10,"loop_item":false}}` → `Some`.
  - `auto_next_turns_slides_on_interval`: reconcile at t0, `on_committed(v)` at t0 → `due` is None at t0 + 7.9 s and `Some(req{from_slide: 0})` at t0 + 8 s; `auto_next_step` → `Goto(1)`.
  - `loops_to_first_slide`: on slide 2 → `Goto(0)`.
  - `stops_on_last_without_loop`: `StopOnLast`, then `stop_on_last()` → `due()` stays `None` and `readout()` is `None`.
  - `stopped_auto_rearms_only_on_visual_change`: after the stop, reconcile with `version + 1` and the same `visual_version` → still stopped; with a new `visual_version` → armed.
  - `never_crosses_into_next_item`: the only results are `Goto(i < slides)` or `StopOnLast`.
  - `manual_next_restarts_interval`: a new `visual_version` at t0 + 5 s → `due = t0 + 13 s` (after its commit).
  - `clear_slide_stops_until_next_show`: `slide_layer_up: false` → `run == None`. Up again with a new `visual_version` → armed with the full interval.
  - `commit_restarts_due_time`: armed at t0, commit at t0 + 3 s → due t0 + 11 s.
  - `commit_at_or_after_waiting_for_restarts_due`: `waiting_for: 5`, `on_committed(6)` → restarted; `on_committed(4)` → not.
  - `does_not_fire_while_waiting_for_commit`: armed at t0 with no commit; at t0 + 20 s, `due()` is `None`.
  - `auto_next_ignored_after_operator_moved`: `cursor_slide` changed → `Ignore`.
  - `message_show_does_not_restart_interval`: `version + 1` with the same visual → `due` unchanged.
  - `one_slide_item_never_arms`.
  - `video_item_ignores_advance` (`slide_layer_up: false`).
  - `adoption_does_not_rearm`.
  - `on_adopt_remaps_auto_run_slide`: map `[Some(0), None, Some(1)]` with run slide 2 → slide 1; run slide 1 → removed.
  - `on_adopt_drops_stale_auto_next`: after a remap, a req with the old `from_slide` → `Ignore`.
  - `pause_and_resume_rearms_full_interval`.
  - `pause_ends_when_another_item_goes_live`.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::advance`. Expected: FAIL to compile (`cannot find type AutoAdvance`, …).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 21 tests`, all passed.
- [ ] **Step 5: Commit** `feat(automation): auto-advance state machine (arm on visual change, wait for commit, loop)`

### Task A10: Pre-service builder and the guarded end

**Files:**
- Create: `src/automation/preservice.rs`
- Modify: `src/automation/mod.rs` (`pub mod preservice;`), `src/strings/automation.rs`

**Interfaces:**
- Consumes: `MacroStep`, `ItemTarget`, `Macro`, `MacroColor`, `TimerDef`, `TimerKind`, `TimeTarget`, `OverlayPlace`, `Builtin`, `PRE_SERVICE_TIMER_ID`.
- Produces:
  - `#[serde(default)] pub struct PreService { pub loop_title: Option<String>, pub music_title: Option<String>, #[serde(deserialize_with = "crate::lenient::or_default")] pub place: OverlayPlace, pub caption: String, pub end: PreServiceEnd }`. `Default` has `caption = SERVICE_STARTS_IN` and `place = TopRight`. `pub fn default_place(has_loop: bool) -> OverlayPlace` gives TopRight with a loop and CenterLarge without one.
  - `pub enum PreServiceEnd { #[default] #[serde(other)] Hide, Logo, ClearAll, FirstItem }` (snake_case).
  - `pub const PRE_SERVICE_MACRO_ID: Uuid`.
  - `pub fn build(p: &PreService) -> Vec<MacroStep>`:
    1. `ShowItem{Titled(music)}` if set;
    2. `ShowItem{Titled(loop)}` if set;
    3. `TimerRestart{PRE_SERVICE_TIMER_ID}`;
    4. `TimerShow{PRE_SERVICE_TIMER_ID}`.

    Every `slide` is 0.
  - `pub fn timer_def(p: &PreService) -> TimerDef`: name "Pre-service", `UntilTime{ServiceStart}`, `Overrun::Stop`, `AtZero::Nothing`, `p.place`, `p.caption`, `builtin: Some(PreService)`, and every flag false.
  - `pub fn macro_def(p: &PreService, existing: Option<&Macro>) -> Macro`: name "Start countdown", Amber, `builtin: Some(PreService)`, steps from `build`. It keeps `existing.remote` and `existing.color`.
  - `#[derive(Clone, Debug, PartialEq)] pub struct PreRun { pub manual_version: u64, pub loop_item: Option<Uuid>, pub timer_run: u64 }`
  - `pub struct EndInput { pub timer_run: u64, pub manual_version: u64, pub cursor_item: Option<Uuid>, pub visual_live: bool }`
  - `#[derive(Debug, PartialEq)] pub struct EndPlan { pub extra: Option<PreServiceEnd>, pub notice: Option<&'static str> }`. The hide always happens, so it is not in the plan.
  - `pub fn end_plan(end: PreServiceEnd, pre: Option<&PreRun>, input: &EndInput) -> EndPlan`:
    - `Hide` → `{None, None}`.
    - Otherwise `{Some(end), None}` only if all of these hold: `pre` is `Some`, `pre.timer_run == input.timer_run`, `pre.manual_version == input.manual_version`, and either `(pre.loop_item.is_some() && input.cursor_item == pre.loop_item)` or `!input.visual_live`.
    - Anything else → `{None, Some(COUNTDOWN_ENDED_NOTHING_ELSE)}`.
- Strings: `SERVICE_STARTS_IN = "Service starts in"`, `PRE_SERVICE_TIMER_NAME = "Pre-service"`, `START_COUNTDOWN = "Start countdown"`, `PRESERVICE_LOCKED = "Change it in the Pre-service tab"`, `COUNTDOWN_ENDED_NOTHING_ELSE = "Countdown ended — nothing else changed because the service has started."`, `RESTART_FOR_NEW_TIME = "Restart the countdown to use the new time."`.

- [ ] **Step 1: Write the failing tests** in `src/automation/preservice.rs`, and add `pub mod preservice;` to `src/automation/mod.rs` in this step:
  - `build_puts_music_before_loop`: both titles set → `[ShowItem Titled("Walk-in music"), ShowItem Titled("Announcements"), TimerRestart PRE, TimerShow PRE]`. With neither title set → only the two timer steps.
  - `preservice_missing_fields_default`: `{}` → caption "Service starts in", `Hide`, `TopRight`. `{"end":"fireworks"}` → `Hide`. `{"place":{"anchor":"center","width":80}}` → `TopRight`.
  - `timer_def_is_until_service_start_and_stops`.
  - `macro_def_keeps_remote_and_colour`.
  - `preservice_end_first_item_when_untouched`: the same run and manual version, with the cursor on the loop item → `extra: Some(FirstItem)`. With nothing visual live → also `Some`.
  - `preservice_end_does_nothing_after_operator_took_an_item`: the manual version differs → `{None, Some("Countdown ended — nothing else changed because the service has started.")}`. The same applies when the cursor is on another item while something visual is live.
  - `preservice_end_after_restart_only_hides`: `pre: None` → the notice.
  - `preservice_end_other_timer_run_only_hides`.
  - `hide_end_never_notices`.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::preservice`. Expected: FAIL to compile (`cannot find type PreService`, …).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 9 tests`, all passed.
- [ ] **Step 5: Commit** `feat(automation): pre-service macro builder and guarded end rule`

### Task A11: `Automation` root, seeding and the pre-service apply

**Files:**
- Modify: `src/automation/mod.rs`, `src/strings/automation.rs`

**Interfaces:**
- Consumes: everything from A2–A10.
- Produces, in `src/automation/mod.rs`:
  - `#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)] #[serde(default)] pub struct Automation { pub timers: Vec<TimerDef>, pub messages: Vec<MessageTemplate>, pub macros: Vec<Macro>, pub pre_service: Option<PreService>, pub seeded: bool }`.
  - `pub fn seed(a: &mut Automation) -> bool` (Plan decision 12: it takes `&mut Automation`, not the spec's `&mut Data`). If `seeded` is false, it appends the built-in timers (Plan decision 8: Service start `UntilTime{ServiceStart}`/Stop; Sermon `Duration{1800}`/CountUp/`on_stage`; Stopwatch `CountUp`; all `OverlayPlace::TopRight`) and `messages::builtin()`, sets `seeded = true` and returns true. Otherwise it returns false. Part B calls it from `Data::after_load` as `automation::seed(&mut self.automation)`.
  - `pub fn apply_preservice(a: &mut Automation, p: PreService)`. It stores `p`. It replaces or inserts the timer `PRE_SERVICE_TIMER_ID` from `preservice::timer_def`. It replaces or inserts the macro `PRE_SERVICE_MACRO_ID` from `preservice::macro_def(&p, existing)` and moves it to index 0.
- Strings: the built-in timer names `SERVICE_START_NAME = "Service start"`, `SERMON_NAME = "Sermon"`, `STOPWATCH_NAME = "Stopwatch"`.

- [ ] **Step 1: Write the failing tests** in a `#[cfg(test)] mod tests` at the bottom of `src/automation/mod.rs` (the module is already declared, so the tests compile and fail on the missing items):
  - `seed_uses_fixed_ids_and_saves_once`: on `Automation::default()`, `seed` returns true. The timer ids are `[SERVICE_START_ID, SERMON_ID, STOPWATCH_ID]`, the message ids `[NURSERY_ID, VEHICLE_ID, CUSTOM_ID]`, and Sermon is `Duration{1800}` + CountUp + `on_stage`. A second `seed` returns false and changes nothing.
  - `seed_not_rerun_after_delete`: remove Sermon, then `seed` → false, and Sermon is still gone.
  - `automation_missing_fields_default`: `{}` → default. `{"timers":[{"kind":{"type":"warp"}}],"macros":[{"steps":[{"step":"x"}]}],"zzz":1}` loads. Its re-serialised `kind` and step equal the input values.
  - `automation_round_trips`: the seeded value plus `apply_preservice` → JSON → equal.
  - `apply_preservice_creates_locked_macro_first`: with 3 macros already present, the pre-service macro is at index 0 with `builtin: Some(PreService)`, and the timer `PRE_SERVICE_TIMER_ID` exists.
  - `apply_preservice_twice_updates_in_place`: still one macro and one timer, and `remote` is kept.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::tests`. Expected: FAIL to compile, `cannot find type Automation` / `cannot find function seed`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 6 tests`, 6 passed.
- [ ] **Step 5: Commit** `feat(automation): Automation root, seeding with fixed ids, pre-service apply`

### Task A12: Save validation, "used by" questions and slot keys

**Files:**
- Create: `src/automation/validate.rs`, `src/automation/slots.rs`
- Modify: `src/automation/mod.rs` (`pub mod validate; pub mod slots;`), `src/strings/automation.rs`

**Interfaces:**
- Consumes: `Automation` (A11), `messages::{parse, fill}`, `Macro::is_overlay_only`, `TimerKind`, `AtZero`.
- Produces in `validate.rs`:
  - Limits: `MAX_TIMERS = 50`, `MAX_MESSAGES = 100`, `MAX_MACROS = 100`, `MAX_STEPS = 30`, `NAME_MAX = 40`.
  - `pub fn validate_timer(a: &Automation, t: &TimerDef) -> Result<(), String>`:
    - name 1–40 `chars` after trimming (`NAME_LENGTH`);
    - `Duration` 1..=86_399 (`DURATION_RANGE`);
    - a new id beyond 50 → `TOO_MANY_TIMERS`;
    - `AtZero::RunMacro`: the macro must exist (`AT_ZERO_MACRO_MISSING`) and be `is_overlay_only` (`AT_ZERO_OVERLAY_ONLY`).
  - `pub fn validate_message(a: &Automation, m: &MessageTemplate) -> Result<Option<&'static str>, String>`: name, count (`TOO_MANY_MESSAGES`), and `Ok(parse(text).warning)`.
  - `pub fn validate_macro(a: &Automation, m: &Macro) -> Result<(), String>`:
    - name;
    - `steps.len() <= 30` (`TOO_MANY_STEPS`);
    - count (`TOO_MANY_MACROS`);
    - saving over a stored macro whose `builtin == Some(PreService)` → `PRESERVICE_LOCKED`;
    - each known `MessageShow` step's values must `fill` its template, and the error is returned as is.
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct Uses { pub macros: usize, pub timers: usize }`.
  - `pub fn uses(a: &Automation, id: Uuid) -> Uses`: `macros` = the number of macros with a step that names `id` (timer, template or overlay); `timers` = the number of timers whose `at_zero` is `RunMacro{macro_id: id}`.
  - `pub fn delete_question(name: &str, u: Uses) -> Option<String>`: `(0, 0)` → `None`; `(m, 0)` → `used_by(name, m)`; `(0, t)` → `used_at_zero(name, t)`; `(m, t)` → `used_by_both(name, m, t)`.
- Produces in `slots.rs` (FEAT-1 `SlotKey` is a `String`):
  - `pub fn timer_key(id: Uuid) -> String` → `"timer:<hyphenated lowercase uuid>"`; `pub fn timer_id(key: &str) -> Option<Uuid>`.
  - `pub const MESSAGE_KEY: &str = "message"`, `pub const TIMER_Z_BASE: u32 = 1_000_000`, `pub const MESSAGE_Z: u32 = u32::MAX`, `pub fn timer_z(index: usize) -> u32`, `pub const MAX_ROOM_TIMERS: usize = 4`.
  - `pub fn room_timer_room<'a>(keys: impl IntoIterator<Item = &'a str>, showing: &str) -> Result<(), &'static str>`: `Ok` when `showing` is already among the keys (an idempotent replace, as FEAT-1's `turn_on` cap treats a key that is already on); otherwise `Ok` only when fewer than 4 keys start with `timer:`, else `TOO_MANY_ROOM_TIMERS`.
- Strings:
  - `NAME_LENGTH = "Give it a name of 1 to 40 letters."` *(plan copy)*
  - `TOO_MANY_TIMERS = "You can have up to 50 timers."` *(plan copy)*
  - `TOO_MANY_MESSAGES = "You can have up to 100 messages."` *(plan copy)*
  - `TOO_MANY_MACROS = "You can have up to 100 macros."` *(plan copy)*
  - `TOO_MANY_STEPS = "A macro can have up to 30 steps."` *(plan copy)*
  - `AT_ZERO_MACRO_MISSING = "That macro was deleted. Pick another one."` *(plan copy)*
  - `AT_ZERO_OVERLAY_ONLY = "A timer can only run a macro that shows or hides things (timers, messages, overlays)."`
  - `TOO_MANY_ROOM_TIMERS = "Up to 4 timers can be on the screen at once."`
  - `used_by(name, n)` → "“{name}” is used by {n} macro(s). Delete it anyway? Those steps will stop working.", with "1 macro" or "N macros".
  - `used_at_zero(name, n)` → "“{name}” runs when {n} timer(s) reach zero. Delete it anyway? Those timers will do nothing at zero.", with "1 timer reaches" or "N timers reach" *(plan copy)*.
  - `used_by_both(name, m, t)` → "“{name}” is used by {m} macro(s) and runs when {t} timer(s) reach zero. Delete it anyway? Those steps will stop working.", with the same singular/plural rules *(plan copy)*.

- [ ] **Step 1: Write the failing tests** in the two new files, and add `pub mod validate; pub mod slots;` to `src/automation/mod.rs` in this step:
  - `validate`:
    - `locked_macro_save_refused` → `PRESERVICE_LOCKED`.
    - `at_zero_rejects_relative_macro_at_save`: a timer whose at-zero runs a macro with a `Next` step → `Err(AT_ZERO_OVERLAY_ONLY)`; an overlay-only macro → `Ok`; a deleted macro id → `Err(AT_ZERO_MACRO_MISSING)`.
    - `validation_limits`: a 41-letter name, `secs` 0 and 86 400, 31 steps, a 51st timer, a 101st message and a 101st macro each fail with their string. A 40-letter name and `secs` 86 399 pass.
    - `message_save_returns_brace_warning`.
    - `macro_save_requires_message_values`: a MessageShow for Nursery with no values → `Err("Fill in “Number”.")`.
    - `used_by_counts_macro_steps_and_at_zero`: Sermon used by 2 macros → `Uses{macros: 2, timers: 0}` and `Some("“Sermon” is used by 2 macros. Delete it anyway? Those steps will stop working.")`; 1 → "used by 1 macro."; 0 → `None`.
    - `macro_used_by_timer_at_zero_says_timer`: a macro run by one timer's at-zero and named by no other macro → `Uses{macros: 0, timers: 1}` and exactly "“Walk-in” runs when 1 timer reaches zero. Delete it anyway? Those timers will do nothing at zero."; run by 2 timers → "… runs when 2 timers reach zero. …". Used by one macro and one timer → "“Walk-in” is used by 1 macro and runs when 1 timer reaches zero. Delete it anyway? Those steps will stop working."
  - `slots`:
    - `timer_key_format_and_z`: `timer_key(id) == format!("timer:{id}")`, `timer_id(&timer_key(id)) == Some(id)`, `timer_id("message") == None`, `timer_z(2) == 1_000_002`.
    - `fifth_timer_in_room_refused` (pure part): with 4 `timer:` keys plus `"message"` and `"saved:…"`, showing a fifth → `Err(TOO_MANY_ROOM_TIMERS)`; with 3 → `Ok`.
    - `reshowing_a_room_timer_is_allowed`: 4 timers on, `showing` = one of them → `Ok`.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::validate`, then `automation::slots`. Expected: FAIL to compile (missing items).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both commands. Expected: `running 7 tests` and `running 3 tests`, all passed.
- [ ] **Step 5: Commit** `feat(automation): save validation, used-by questions and slot keys`

### Task A13: Runtime and message bookkeeping

**Files:**
- Create: `src/automation/runtime.rs`
- Modify: `src/automation/mod.rs` (`pub mod runtime;`)

**Interfaces:**
- Consumes: `TimerRun`, `TimerDef`, `TimersView`, `AutoReadout`, `AutoState` (A9), `PreRun`, `MessageTemplate`.
- Produces:
  - `pub type EngineKey = &'static str; pub const ROOM_ENGINE: EngineKey = "room";`. Part B (B4) changes the alias to FEAT-6's `EngineId` when it exists; no other line changes, because nothing below depends on the key type beyond `Eq + Hash + Copy`.
  - `#[derive(Default)] pub struct AutomationRuntime { pub timers: HashMap<Uuid, TimerRun>, pub auto: HashMap<EngineKey, AutoState>, pub hide_at: HashMap<Uuid /* shown */, DateTime<Utc>>, pub pre: Option<PreRun>, pub last_message: Option<Uuid> }`.
  - `pub enum Due { TimerAtZero { timer: Uuid, run: u64 }, PreServiceEnd { run: u64 }, AutoHideMessage { shown: Uuid } }`. Part B maps these one-to-one onto `InternalCommand`.
  - `impl AutomationRuntime`:
    - `pub fn run_mut(&mut self, id: Uuid) -> &mut TimerRun`, which creates a default entry if there is none.
    - `pub fn view<Tz: TimeZone>(&self, defs: &[TimerDef], now, tz: &Tz, starts_at: Option<TimeOfDay>, auto: Option<AutoReadout>) -> TimersView`: in `defs` order, using a default run when there is none. Unknown kinds and runs whose id is not in `defs` are left out.
    - `pub fn zero_crossings(&mut self, defs: &[TimerDef], now) -> Vec<Due>`: calls `check_zero` for each def. A `PreService` builtin → `PreServiceEnd{run}`. `AtZero::HideFromRoom` or `RunMacro` → `TimerAtZero{timer, run}`. `Nothing` or Unknown → nothing.
    - `pub fn message_shown(&mut self, template: Uuid, shown: Uuid, hide_after_secs: Option<u32>, now)` (spec §5.7): `last_message = Some(template)`; with `Some(s)`, `hide_at.insert(shown, now + s)`.
    - `pub fn due_hides(&mut self, now) -> Vec<Due>`: every `hide_at` entry with `at <= now` becomes `AutoHideMessage{shown}` and is removed, so it is emitted once.
    - `pub fn auto_hide_applies(shown: Uuid, frame_message: Option<Uuid>) -> bool` (associated fn): true only when the frame's message slot still holds that `shown`. A replaced or cleared message is never hidden by an old timer.
    - `pub fn prune(&mut self, defs: &[TimerDef], shown_in_frames: &[Uuid])`: drops `hide_at` entries not in any frame and runs of deleted timers.
    - `pub fn is_idle(&self, defs: &[TimerDef], room_timers: &[Uuid], watching: bool) -> bool` (Plan decision 15): false when an existing timer is Running/RunningTo, when `hide_at` is not empty, when any `AutoState` has a run with `due.is_some()`, or when a **Stopped `UntilTime`** timer's readout is visible: it is in `room_timers` (a `timer:` slot in any frame), it is `on_remote`, or `watching` is true (the Automation dialog is open). Otherwise true.
    - `pub fn reset_timer(&mut self, id: Uuid)`: resets the run and clears `pre` when `id == PRE_SERVICE_TIMER_ID`.
    - `pub fn show_message_template(&self, templates: &[MessageTemplate]) -> Option<Uuid>`: `last_message` if it still exists, else the first template.
    - `pub fn on_adopt(&mut self, item_id: Uuid, map: &[Option<usize>])` (Plan decision 14): calls `AutoState::on_adopt(item_id, map)` on every engine's state. This is the spec's `Automation::on_adopt`; FEAT-4 Part B calls `app.automation.lock().on_adopt(item_id, &map)`.

- [ ] **Step 1: Write the failing tests** in `src/automation/runtime.rs`, and add `pub mod runtime;` to `src/automation/mod.rs` in this step:
  - `view_in_definition_order_without_unknown_kinds`;
  - `view_skips_runs_of_deleted_timers` (Review Focus 5);
  - `zero_crossings_ignore_deleted_timers` (Review Focus 5);
  - `zero_crossing_emits_at_zero_once` (`HideFromRoom` → one `TimerAtZero{run: 0}`, and nothing on the next call);
  - `nothing_at_zero_emits_nothing`;
  - `preservice_timer_emits_preservice_end`;
  - `message_hide_after_sets_hide_at`: `message_shown(NURSERY_ID, s1, Some(30), t0)` → `last_message == Some(NURSERY_ID)`, `hide_at[s1] == t0 + 30 s`; `None` → no entry, `last_message` still set;
  - `due_hides_emits_once`: at t0 + 29 s nothing; at t0 + 30 s one `AutoHideMessage{shown: s1}`; at t0 + 31 s nothing;
  - `auto_hide_ignores_replaced_message` (pure part): `auto_hide_applies(s1, Some(s2)) == false`, `auto_hide_applies(s1, None) == false`, `auto_hide_applies(s1, Some(s1)) == true`;
  - `due_hides_and_prune` (a `hide_at` entry whose `shown` is not in the frames is dropped);
  - `idle_only_when_nothing_runs_or_waits`;
  - `stopped_until_time_readout_keeps_ticker_awake_only_when_visible`: a Stopped `UntilTime{ServiceStart}` timer with every flag false → idle; the same timer in `room_timers`, or with `on_remote`, or with `watching = true` → not idle; a Stopped `Duration` timer in `room_timers` → idle (its readout does not change);
  - `reset_preservice_timer_clears_pre`;
  - `show_message_opens_on_last_template_or_first`;
  - `runtime_on_adopt_remaps_every_engine`: two engines (`"room"`, `"practice"`) both on item `a` at slide 2, map `[Some(0), None, Some(1)]` → both at slide 1; an engine on item `b` is untouched.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::runtime`. Expected: FAIL to compile (`cannot find type AutomationRuntime`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: `running 15 tests`, 15 passed.
- [ ] **Step 5: Commit** `feat(automation): runtime, message bookkeeping and on_adopt`

### Task A14: Ticker publish rule and phone payloads

**Files:**
- Create: `src/automation/ticker.rs`, `src/automation/remote.rs`
- Modify: `src/automation/mod.rs` (`pub mod ticker; pub mod remote;`), `src/strings/automation.rs`

**Interfaces:**
- Consumes: `TimersView`, `Automation`, `messages::parse`.
- Produces:
  - `ticker.rs`:
    - `pub const WAKE_MS: i64 = 200`, `pub const MIN_GAP_MS: i64 = 250`.
    - `pub fn wake_after(idle: bool) -> Option<i64>`: `None` while idle (sleep on the `Notify`), else `Some(200)`.
    - `#[derive(Default)] pub struct Publisher { last: Arc<TimersView>, last_at: Option<DateTime<Utc>> }` with `pub fn offer(&mut self, view: TimersView, now) -> Option<Arc<TimersView>>` and `pub fn current(&self) -> Arc<TimersView>`. `offer` publishes when `view != *last` and (`last_at` is `None` or `now − last_at >= 250 ms`). `current()` returns the same `Arc` the last publish returned.
  - `remote.rs`:
    - `pub fn timers_message(view: &TimersView) -> serde_json::Value`: `{"type":"timers","timers":[{"name","text","over"}]}` for the `on_remote` timers only.
    - `pub fn macro_buttons(a: &Automation) -> Vec<serde_json::Value>`: `[{id,name,color}]` for `remote` macros, in order.
    - `pub fn message_buttons(a: &Automation) -> Vec<serde_json::Value>`: `[{id,name,fields}]` for `remote` templates.
    - `pub fn macro_allowed(a: &Automation, id: Uuid) -> bool`, `pub fn message_allowed(a: &Automation, id: Uuid) -> bool`.
- Strings: `TOO_FAST = "Too fast — try again."`, `NOT_ALLOWED_FROM_PHONE = "Not allowed from the phone."`, `confirm_run(name)` → "Run “{name}”?".

- [ ] **Step 1: Write the failing tests** in the two new files, and add `pub mod ticker; pub mod remote;` to `src/automation/mod.rs` in this step:
  - `ticker`:
    - `no_timers_update_without_text_change`: the same view twice → the second offer is `None`.
    - `changes_are_throttled_to_250ms`: a change 100 ms after a publish → `None`; the same view 250 ms after the publish → `Some`.
    - `updates_at_least_once_a_second_while_running`: a running CountUp timer, offered every 200 ms from 0 to 5000 ms (view rebuilt each time) → publishes at 0, 1000, …, 5000 (6), and no gap is over 1000 ms.
    - `current_is_last_published_arc`: `Arc::ptr_eq(&published, &publisher.current())`.
    - `sleeps_when_idle`: `wake_after(true) == None`, `wake_after(false) == Some(200)`.
  - `remote`:
    - `remote_timers_filtered_to_on_remote`: the exact JSON `{"type":"timers","timers":[{"name":"Sermon","text":"29:59","over":false}]}`.
    - `remote_lists_only_marked_macros_and_messages`, where the Nursery `fields` are `["Number"]`.
    - `allowed_checks_flag_and_existence`.
    - `phone_strings_are_exact`: `TOO_FAST`, `NOT_ALLOWED_FROM_PHONE` and `confirm_run("Walk-in") == "Run “Walk-in”?"`.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::ticker`, then `automation::remote`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both commands. Expected: `running 5 tests` and `running 4 tests`, all passed.
- [ ] **Step 5: Commit** `feat(automation): ticker publish rule and phone payloads`

### Task A15: Operator strip rules

**Files:**
- Create: `src/automation/strip.rs`
- Modify: `src/automation/mod.rs` (`pub mod strip;`)

**Interfaces (spec §3.1, §7; B9 re-exports these from `src/ui/operator/automation.rs`):**
- `#[derive(Clone, Debug, PartialEq)] pub struct TimerChip { pub id: Uuid, pub name: String, pub text: String, pub over: bool, pub running: bool, pub in_room: bool }`
- `#[derive(Clone, Debug, PartialEq)] pub struct MacroBtn { pub id: Uuid, pub name: String, pub color: MacroColor, pub locked: bool /* builtin == PreService */ }`
- `pub fn timer_chips(view: &TimersView, in_room: &[Uuid]) -> (Vec<TimerChip>, usize /* +N */)`: a timer qualifies when its state is Running, Paused or Finished, or its id is in `in_room`, in view (Timers-tab) order. Up to 2 qualify → all, `0`. More → the first only, and `N = qualifying − 1` (the second slot becomes "+N timers").
- `pub fn timers_more_label(n: usize) -> String` → "+{n} timers" (`"+1 timer"` for 1).
- `pub fn macro_buttons(macros: &[Macro]) -> (Vec<MacroBtn>, bool /* More… */)`: 6 or fewer → all, `false`; more → the first 5, `true`. The order is kept, so the pre-service macro (index 0 after `apply_preservice`) is first.
- `pub const TWO_ROW_MAX_HEIGHT: f32 = 800.0` (Plan decision 16). `pub fn strip_rows(window_height: f32, nonempty: [bool; 4]) -> [bool; 4]`: rows that are empty are never shown. Above 800 px every non-empty row shows. At 800 px or less, Row 1 (if non-empty) and then the first non-empty of Rows 2–4 show, and nothing else.
- `pub fn auto_label(r: &AutoReadout) -> String` → `"Auto · 0:05"`, using `timers::text(next_in_secs × 1000)`.

- [ ] **Step 1: Write the failing tests** in `src/automation/strip.rs`, and add `pub mod strip;` to `src/automation/mod.rs` in this step:
  - `strip_overflow_goes_to_more_menu`: 8 macros → 5 buttons + `true`; 6 → 6 + `false`; 3 qualifying timers → 1 chip + 2, and `timers_more_label(2) == "+2 timers"`, `timers_more_label(1) == "+1 timer"`.
  - `stopped_timers_only_chip_when_in_room`: a Stopped timer not in the room → no chip; the same timer in `in_room` → a chip.
  - `preservice_macro_is_first_and_locked`.
  - `strip_max_two_rows_at_768`: `strip_rows(768.0, [true, true, true, true]) == [true, true, false, false]`; `[true, false, true, true]` → `[true, false, true, false]`; `strip_rows(1080.0, [true, true, false, true]) == [true, true, false, true]`.
  - `auto_label_format`: `next_in_secs: 5` → `"Auto · 0:05"`; 75 → `"Auto · 1:15"`.
- [ ] **Step 2: Run** `& $FC test --release --lib automation::strip`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `& $FC test --release --lib automation::strip` (`running 5 tests`, all pass), then `& $FC test --release --lib automation`, `& $FC test --release --lib timers` and `& $FC test --release --lib lenient`, then `& $FC test --release --lib`. Expected: all pass, with no existing test broken.
- [ ] **Step 5: Commit** `feat(automation): pure operator strip rules`

---

# PART B — Integration after Plans 3 and 4 merge

> **BLOCKED** until `native-foundation` contains Plan 3 (native operator) and Plan 4 (imports, recovery, packaging; Tauri and the web operator removed), and FEAT-1 Part B (`src/theme/overlay/mod.rs` with `OverlaySlot`/`OverlaySource`/`BoundCtx`/`bound_text`, `src/theme/overlay/slots.rs` with `show`/`hide`/`retain`/`turn_on`/`check_seen_clear`, `src/theme/paint/overlay.rs` with `draw`/`OverlayKey`, `Frame.overlays`, `Live.clear_epoch`, `Data::after_load`, the merge-by-key rule, the FEAT-1 key table). FEAT-1 re-exports these as `crate::overlay` and `crate::render::overlay`; this plan names the real files. Before starting, rebase this branch onto `native-foundation` and run `cargo test`: Part A must still pass unchanged. Paths below are Plan 3's layout. From here on, tests run with the repo's normal `cargo test`; several filters go after `--` (`cargo test -- a:: b::`), because cargo takes only one positional filter.

### Task B1: Final module paths, model fields and seeding on load

**Files:**
- Modify: `src/lib.rs` (`pub mod strings;` only if FEAT-1/FEAT-4 have not added it yet; `lenient` and `timers` are already declared by A1/A2), `src/strings/mod.rs` (`pub mod automation;`), `src/automation/mod.rs` (replace `#[path = "../strings/automation.rs"] pub mod strings;` with `pub use crate::strings::automation as strings;`, so every `crate::automation::strings::…` path keeps working), `src/model.rs`, the `Data::after_load` site (FEAT-1), the `service/update` / item save / "copy last week" actions in `src/app.rs` (or wherever Plan 4 moved them)

**Interfaces:**
- `model.rs` re-exports `automation::{Automation, MessageTemplate, Macro, MacroStep, PreService}`, `advance::AutoAdvance` and `timers::{TimerDef, TimeOfDay}`. The new fields: `Data.automation: Automation` (`#[serde(default)]`); `Service.starts_at: Option<TimeOfDay>` (`#[serde(default, deserialize_with = "crate::timers::lenient_time")]`); `ServiceItem.advance: Option<AutoAdvance>` (`#[serde(default, deserialize_with = "crate::automation::advance::lenient_advance", skip_serializing_if = "Option::is_none")]`, so a bad value drops the field instead of failing `Store::load`, spec §4.1); `LiveItem.advance: Option<AutoAdvance>` (the same attributes), a snapshot taken at Take Live.
- `Data::after_load` calls `automation::seed(&mut self.automation)` first in its fixed order and ORs the result into its return value (Plan decision 12).
- Merge by key: `service/update` and the item save leave `starts_at`/`advance` alone when the key is missing, and an explicit `null` clears them. **Needs FEAT-6:** "copy last week" and templates copy `starts_at` and `advance`.

- [ ] **Step 1: Write the failing tests:** `old_data_json_loads_with_automation_defaults` (today's v0.2 fixture), `seed_uses_fixed_ids_and_saves_once` (through `Data::after_load` returning true once), `newer_automation_survives_load_and_save` (an unknown step and kind written back byte-equal through `Store`), `bad_advance_on_item_loads_without_it` (a `data.json` with `"advance":{}` and another with `"advance":{"secs":70000}` loads through `Store::load`; the item has `advance == None`), `service_update_without_starts_at_keeps_it`, `item_save_with_null_advance_clears_it`, `live_item_snapshots_advance_at_take_live`, `titles_survive_copy_last_week` (Needs FEAT-6).
- [ ] **Step 2:** `cargo test -- model:: store:: app::` → FAIL.
- [ ] **Step 3:** Implement. `cargo test -- lenient:: timers:: automation::` → all of Part A still passes.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(model): automation data, service start time and item auto-advance`

### Task B2: Live engine — mutate/commit split and batches

**Files:**
- Modify: `src/live.rs`

**Interfaces (spec §5.2; Plan decision 13 for the batch shape):**
- `pub enum Change { None, Silent, Overlay, Visual }` (`Ord`). `pub enum Origin { User, Auto }`.
- `fn mutate(&mut self, step: &LiveStep, plan: &dyn Plan) -> Result<Change, String>`. `go()` no longer bumps or clears `notice`. FEAT-1's mutators return `bool` → `Change::Overlay` or `Change::None`.
- `fn commit_change(&mut self, change: Change, origin: Origin)` follows the class table: Silent → `version + 1`; Overlay → that plus one history snapshot; Visual → that plus `visual_version = version`, plus `manual_version = version` when the origin is `User`, and `notice` cleared.
- `pub enum LiveStep { Cmd(Command), ShowOverlay(OverlaySlot), HideOverlay(SlotKey), ScreenView { screen: OutputRole, view: Uuid } }` (the last only with FEAT-7).
- `pub enum BatchStep { Live { step: LiveStep, show: bool /* PlannedStep::is_show */ }, Runtime(usize) }`. The show flag travels with its step, so the two can't get out of step.
- `pub fn apply_batch(&mut self, env_id: &str, base: Option<u64>, seen_clear: Option<u64>, steps: &[BatchStep], plan: &dyn Plan, origin: Origin, runtime: &mut dyn FnMut(usize) -> Result<(), String>) -> BatchOutcome`:
  1. A seen id returns the remembered outcome.
  2. The base is checked once.
  3. Each step runs against a checkpoint `(frame, cursor, item, next_inst, notice)`. A failure restores the checkpoint and is collected. `Live{show: true, ..}` steps with `seen_clear < clear_epoch` are skipped and collected.
  4. One `commit_change` with the strongest change.
  
  `pub struct BatchOutcome { pub outcome: Outcome, pub failed: Vec<(usize, String)>, pub skipped_shows: Vec<usize> }`.
- New `Live` fields: `visual_version` and `manual_version` (`#[serde(default)]`).

- [ ] **Step 1: Write the failing tests:** `batch_records_one_history_snapshot`, `failed_step_restores_next_inst_and_notice`, `next_after_auto_next_is_stale`, `next_after_clear_all_is_stale`, `silent_bump_keeps_visual_and_history`, `adoption_makes_inflight_next_stale` (Needs FEAT-4; otherwise use a direct Silent commit), `change_classes_match_table`, `batch_skips_show_steps_after_unseen_clear`, `batch_with_no_change_does_not_bump`. The existing `delayed_next_cannot_undo_clear_all`, `two_nexts_from_same_state_advance_once` and every `report`/history test must pass **unchanged**.
- [ ] **Step 2:** `cargo test live::` → FAIL.
- [ ] **Step 3:** Implement. `run()` for a single command is: check → `mutate` → `commit_change`.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `refactor(live): split mutation from commit; Change classes; apply_batch`

### Task B3: Timer and message overlays, commands and the clear matrix

**Files:**
- Modify:
  - `src/theme/overlay/mod.rs` (FEAT-1): add `OverlaySource::Timer { timer_id, caption, large, look }` and `OverlaySource::Message { shown, template_id, text, look }` (spec §4.2), and their arms in **every** `OverlaySource` method:
    - `survives_logo()`: `Timer` → **false**, `Message` → **true** (the clear matrix depends on this);
    - `kind()`: both → `OverlayKind::Other`;
    - `is_bound()`: `Timer` → **true**, `Message` → false;
    - `bound_text(src, ctx)`: `Timer` → `ctx.timers` lookup by `timer_id` → `timers::text(value_ms)`, or `None` when the timer is missing from the view; `Message` → `None` (its text is static);
    - `BoundCtx` gains `pub timers: Arc<TimersView>`.
  - `src/theme/overlay/slots.rs` (FEAT-1): no new mutator; `turn_on`'s 8-saved cap already ignores non-`saved:` keys (timers and messages don't count, FEAT-1 §5.2). Add the test below.
  - `src/theme/paint/overlay.rs` (FEAT-1): `OverlayKey::of` leaves the bound time text **out** of the prepare key for `Timer` (the key holds `source`, `place`, `theme_font`, `canvas`; `text: None`), so a tick never re-prepares the slot; B8 redraws only the time cells.
  - `src/timers.rs` (`OverlayPlace::placement() -> Placement`, FEAT-1 §4.5 mapping), `src/automation/messages.rs` (`MessagePlace::placement() -> Placement`), `src/automation/slots.rs` (`pub fn timer_slot(def: &TimerDef, index: usize, look: OverlayLook) -> OverlaySlot`, `pub fn message_slot(t: &MessageTemplate, shown: Uuid, text: String, look: OverlayLook) -> OverlaySlot`)
  - `src/live.rs` (`Command::{ShowMessage, HideMessage, ShowTimer, HideTimer}` and their `mutate` arms; F1–F6 rows for the timer and message slots)

**Interfaces:**
- The z values come from `slots::timer_z` and `slots::MESSAGE_Z`.
- `ShowMessage`/`ShowTimer` require the envelope's `seen_clear` (FEAT-1 `check_seen_clear`; refused with `CLEARED_TRY_AGAIN` when older). `HideMessage`/`HideTimer` are baseless. `ShowTimer` calls `slots::room_timer_room(frame keys, &timer_key(id))`, so re-showing a timer that is already on is a replace, not a refusal. In the clear matrix, Logo runs FEAT-1's `retain(survives_logo)`, which removes `timer:` slots and keeps `message`; F1 and F6 remove both; F2–F4 keep both.
- The `ShowMessage` mutate arm only builds and shows the slot; the runtime bookkeeping (`hide_at`, `last_message`) is App work in B5.
- **Needs FEAT-6:** the routing table (shows go to the practice engine; hides and clears go to both engines).

- [ ] **Step 1: Write the failing tests:**
  - `src/theme/overlay/mod.rs`: `logo_retain_drops_timer_keeps_message` (`Timer.survives_logo() == false`, `Message.survives_logo() == true`, `Timer.is_bound()`, `!Message.is_bound()`), `timer_bound_text_follows_view` (`value_ms: -72_000` → `"+1:12"`; a timer missing from the view → `None`).
  - `src/theme/overlay/slots.rs`: `timers_and_messages_do_not_count_toward_saved_cap` (8 saved on plus 4 timers and a message → a 9th saved is still refused, and a timer `turn_on` is not).
  - `src/theme/paint/overlay.rs`: `timer_key_ignores_time_text` (two `OverlayKey::of` with texts `"1:11"` and `"1:10"` are equal for a Timer slot, and differ for a Text slot).
  - `src/live.rs`: `clear_all_removes_timer_and_message_overlays`, `f6_removes_timer_and_message`, `logo_removes_timer_keeps_message`, `clear_slide_keeps_overlays`, `incoming_item_keeps_overlays`, `show_message_replaces_previous`, `show_message_after_unseen_clear_refused`, `show_timer_after_unseen_clear_refused`, `hide_after_clear_always_allowed`, `every_clear_bumps_clear_epoch`, `fifth_timer_in_room_refused`, `reshowing_room_timer_replaces_it`, `practice_clears_reach_room` (Needs FEAT-6).
  - `src/timers.rs` / `messages.rs`: `timer_place_maps_to_feat1_placement` (corners → same anchor, width 25, margin 4; `CenterLarge` → Center, width 80; message Bottom/Top → width 100).
- [ ] **Step 2:** `cargo test -- live:: theme::overlay theme::paint::overlay timers:: automation::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(live): timer and message overlays with clear-wins checks`

### Task B4: App runtime, timer controls and editing actions

**Files:**
- Modify: `src/app.rs` (or Plan 4's split action module): `App.automation: Mutex<AutomationRuntime>`, a lock-order doc comment (`data → live → practice → automation`), the actions; `src/automation/runtime.rs` (change `EngineKey` to FEAT-6's `EngineId` once it exists; Needs FEAT-6)

**Interfaces (spec §5.9):** `timer/start|pause|reset|restart {id}` and `timer/set-now {id, secs|at}`. These build a `StartCtx` from `chrono::Local`, the current service's `starts_at` and `live.clear_epoch`, are runtime only, and send `Update::Timers`. Also `automation/timer|message|macro/save|delete`, `automation/macro/move {id,to}`, `automation/preservice/save` (`apply_preservice`), `service/starts-at {id, at|null}` and `auto/set {on}`. Saves use the A12 validators. A delete for which `validate::uses` is non-zero returns the `delete_question` text for the UI to confirm (`{confirm: true}` deletes). Deleting a shown timer or message hides it first. The phone may not call any of these.

- [ ] **Step 1: Write the failing tests:** `timer_actions_round_trip_state`, `set_for_now_prompt_changes_run_only`, `start_countdown_after_start_time_refused`, `save_validation_messages_reach_caller`, `delete_referenced_timer_asks_first`, `delete_macro_used_at_zero_asks_with_timer_wording`, `delete_shown_message_hides_it_first`, `preservice_save_creates_locked_macro_first_in_strip`, `starts_at_null_clears`.
- [ ] **Step 2:** `cargo test app::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(app): automation runtime, timer controls and editing actions`

### Task B5: Running macros, `InternalCommand`, messages and at-zero rules

**Files:**
- Modify: `src/live.rs` (`Command::RunMacro { macro_id }`), `src/app.rs` (`pub enum InternalCommand` — **no** `Deserialize`; `App::command_internal`; `App::command` resolves RunMacro before the engine; the ShowMessage bookkeeping), `src/automation/macros.rs` (`pub fn to_batch(&Resolved, ..) -> Vec<BatchStep>`, mapping `PlannedStep` → `BatchStep::Live { step, show: planned.is_show() }` or `BatchStep::Runtime(i)`; switch `ScreenView.screen` to `OutputRole` per Plan decision 9)

**Interfaces:** `InternalCommand { AutoNext { engine: EngineId, item_id, from_slide, armed_visual }, AutoHideMessage { shown }, TimerAtZero { timer, run }, PreServiceEnd { run } }` with fresh ids `auto-<uuid>`. The flow is spec §5.3 steps 1–4:
1. `resolve`.
2. `apply_batch` with Runtime steps applied to the locked runtime.
3. The Action steps after the commit. **Needs FEAT-3:** `App::stage_message_show(text, MsgSource::Macro)`, and `do_action` for the stage hide and layout steps. Until then these steps fail with `ran_but_step_failed`.
4. One toast or notice from `strings`.

A relative macro needs a fresh base (`STALE_TRY_AGAIN`). Other macros carry `seen_clear`. At zero:
- Re-check `is_overlay_only` (`at_zero_changes_slides`, A8).
- Run with `TimerRun.seen_clear` (`at_zero_skipped_shows`, A8).
- `HideFromRoom` hides the slot.

**Messages (spec §5.7).** Whenever a `ShowMessage` (operator, phone or macro step) is accepted and committed, the App, still holding the locks, calls `runtime.message_shown(template_id, shown, template.hide_after_secs, now)`, which sets `hide_at[shown]` and `last_message`, and notifies the ticker. `InternalCommand::AutoHideMessage{shown}` reads the frame's message slot and hides it only when `AutomationRuntime::auto_hide_applies(shown, frame_message_shown)` is true; otherwise it is a no-op with no version bump. After every reconcile, `runtime.prune(defs, shown_in_frames)` drops stale `hide_at` entries.

- [ ] **Step 1: Write the failing tests:** `macro_is_one_version_bump`, `deleted_target_runs_nothing`, `runtime_step_failure_continues_and_toasts`, `refused_timer_target_runs_nothing`, `action_steps_run_after_commit_and_report_failure`, `stage_message_step_uses_msg_source_macro` (Needs FEAT-3), `stage_layout_auto_step_runs` (Needs FEAT-3), `same_id_runs_once_new_id_runs_again`, `relative_macro_without_base_rejected`, `overlay_only_macro_needs_no_base`, `non_relative_macro_skips_show_steps_after_clear`, `at_zero_macro_skips_show_steps_after_clear_since_start`, `at_zero_skips_macro_made_relative_later`, `message_hide_after_auto_hides` (Nursery with `hide_after_secs: Some(15)`, `FakeClock` + 15 s → the message slot is gone, one version bump), `auto_hide_ignores_replaced_message` (show A with 15 s, show B with no auto-hide at +5 s, +15 s → B still up, no bump), `clear_all_clears_message_shown_flag` (show, F1 → `remote_view().message_shown == false`, the Message button is not accent, and `hide_at` is empty after the reconcile), `show_message_sets_last_message`, `internal_commands_do_not_deserialize` (a compile-time check: `InternalCommand` has no `Deserialize`, and the JSON `{"cmd":"auto_next"}` fails to parse as `Envelope`).
- [ ] **Step 2:** `cargo test -- app:: live::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(app): run macros atomically; internal commands; message auto-hide; at-zero rules`

### Task B6: Auto-advance hooks and the pre-service end

**Files:**
- Modify: `src/app.rs` (`reconcile` after every accepted command, internal command and `Live::report` that returns true, for each engine through `App::with_engine(id)` (Needs FEAT-6; otherwise the single room engine under `ROOM_ENGINE`); `on_committed` from `Committed(c)`; `AutoNext` applied as `Origin::Auto`; record `runtime.pre` after Start countdown; `PreServiceEnd` uses `preservice::end_plan` and joins the extra step to the hide batch)
- FEAT-4's adoption calls `AutomationRuntime::on_adopt(item_id, &map)` (A13; Plan decision 14) as `app.automation.lock().on_adopt(item_id, &map)`, inside the same lock order. Needs FEAT-4: if FEAT-4 Part B landed first with a `// FEAT-2: on_adopt` marker, replace the marker with that call here.

**Interfaces:** `AdvanceInput` is built from `Live` (`slides` from `LiveItem.slides.len()`, `slide_layer_up` from `frame.slide`, `advance` from `LiveItem.advance`). **With no room connected, a change counts as committed when it is made** (Review Focus 2). `visual_live` for the pre-service end is true when the frame has a slide, media, background or logo.

- [ ] **Step 1: Write the failing tests:** `auto_run_per_engine` (Needs FEAT-6), `failed_revert_rearms_from_reverted_cursor`, `auto_advances_with_room_closed`, `preservice_loop_keeps_running_after_music_starts`, `next_during_preservice_turns_loop_slide`, `preservice_end_first_item_when_untouched`, `preservice_end_does_nothing_after_operator_took_an_item`, `preservice_end_after_restart_only_hides`, `quiet_adoption_remaps_auto_run` (Needs FEAT-4).
- [ ] **Step 2:** `cargo test app::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(app): auto-advance reconcile and guarded pre-service end`

### Task B7: The ticker and `Update::Timers`

**Files:**
- Create: `src/automation/task.rs` (the tokio task; `ticker.rs` stays pure)
- Modify: `src/app.rs` (`Update::Timers(Arc<TimersView>)`, `App::timers() -> Arc<TimersView>`, `App.ticker_wake: Notify`, `App.automation_dialog_open: AtomicBool`, start the task next to the server task), `src/automation/mod.rs`

**Interfaces:** `pub async fn ticker(app: Arc<App>, clock: Arc<dyn Clock>)`. It computes `idle = runtime.is_idle(defs, room_timer_ids, automation_dialog_open)` (Plan decision 15), where `room_timer_ids` are the `slots::timer_id` of every `timer:` slot in any engine's frame. While `wake_after(idle)` is `None` it waits on the `Notify`, which every timer action, show, arm, `ShowMessage` and dialog open notifies. Otherwise it sleeps 200 ms. Each wake runs `zero_crossings`, `due_hides`, each engine's `AutoState::due` → `command_internal`, then `Publisher::offer` → `Update::Timers`. It never bumps `Live.version`.

- [ ] **Step 1: Write the failing tests** (`FakeClock`, paused tokio time): `timers_view_shared_with_stage`, `ticker_fires_at_zero_through_command_internal`, `ticker_sleeps_when_idle_and_wakes_on_start`, `stopped_service_start_in_room_keeps_updating` (a Stopped Service start timer shown in the room: two `Update::Timers` 1 s apart with different text), `ticking_never_bumps_live_version`.
- [ ] **Step 2:** `cargo test automation::task` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(automation): ticker task and Update::Timers`

### Task B8: Drawing timers and messages

**Files:**
- Modify: `src/theme/paint/overlay.rs` (FEAT-1: the `Timer` and `Message` arms of `draw`), `src/theme/overlay/mod.rs` (`BoundCtx.timers` is filled from `App::timers()` by the callers), `src/ui/room.rs`, `src/ui/preview.rs`, FEAT-7's stream renderer if merged (follow `Update::Timers`, re-render only timer slots whose text changed; prepare keys come from `OverlayKey::of`, which already leaves the time text out, B3)

**Interfaces / values (spec §6, at 1080p):**
- **Corner timer:** caption 30 px weight 600 above the time at 72 px weight 700. Plate `#000000` at 65 %, radius 16, padding 16×24, margin 48. Digits use fixed cells as wide as the widest digit.
- **Centre large:** time 220 px weight 700, caption 56 px, no plate, theme shadow.
- **Banner:** a `BoxKind::Bar` at 75 %, 12 px from the edge. Text 52 px weight 600, centred, up to 3 lines, shrinking to no less than 44 px.
- **Font:** `OverlayLook.font`, or the church default theme's font when that is empty.

- [ ] **Step 1: Write the failing golden tests:** `timer_corner_top_right`, `timer_center_large_with_caption`, `message_banner_three_lines_min_44px`, `digits_do_not_shift` (the `1:11` and `1:10` boxes are equal), `overlay_uses_church_default_font`, `logo_screen_still_draws_message_banner` (testing backend on `ui/room.slint`, plus a FEAT-7 compose golden if merged), `failed_message_prepare_drops_only_message_slot`.
- [ ] **Step 2:** `cargo test -- theme::paint::overlay ui::room` → FAIL.
- [ ] **Step 3:** Implement and generate the goldens; check them by eye.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(render): timer and message overlay drawing`

### Task B9: Operator automation strip

**Files:**
- Create: `ui/operator/automation-strip.slint`, `src/ui/operator/automation.rs` (`pub use crate::automation::strip::*;` plus the Slint glue: `apply_timers`, model conversion, callbacks)
- Modify: `ui/operator/live.slint` (host the strip under FEAT-1's overlays strip), `src/ui/operator/window.rs` (forward `Update::Timers` to `apply_timers`), `src/ui/ops.rs` (`outcome_toast(o, from_button)`)

**Interfaces:** the `AutomationStrip` properties, callbacks and element ids are exactly as spec §7 (`auto-strip`, `message-btn`, `auto-chip`, `timer-<n>`, `timers-more`, `macro-<n>`, `macros-more`). Row visibility comes from `strip::strip_rows(window height, …)`; chips from `strip::timer_chips`; buttons from `strip::macro_buttons`; the Auto chip text from `strip::auto_label` (all A15). The Message button reads the frame's `message` slot. Overflowing timers and macros open the More… menus.

- [ ] **Step 1: Write the failing tests** (`Ops::inline`): `strip_shows_running_timer_and_toggle_pauses`, `timer_chip_red_when_over`, `message_button_follows_frame`, `macro_button_runs_and_toasts`, `macro_stale_toasts_try_again`, `macro_failure_shows_notice`, `auto_chip_shows_countdown_and_pauses`, `strip_hides_rows_at_768` (window 1366×768, 4 non-empty rows → elements of rows 3–4 not visible, More… lists them).
- [ ] **Step 2:** `cargo test ui::operator::automation` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): operator automation strip`

### Task B10: Ctrl+Shift+M and the Show message overlay

**Files:**
- Create: `ui/dialogs/show-message.slint`, `src/ui/dialogs/show_message.rs`
- Modify: `src/ui/operator/keys.rs` (`KeyAction::ShowMessage` for Ctrl+Shift+M / Cmd+Shift+M, in FEAT-1's capture set, checked against FEAT-3's `OS_RESERVED`)

**Interfaces:** Show message is an ask-layer overlay (`asks += 1`), so the dialog under it survives. It opens on `runtime.show_message_template(..)`: a template ComboBox, one field per `messages::parse(text).fields`, the `messages::preview` line, the `PRACTICE_WARNING` line while practice mode is on (Needs FEAT-6), and the buttons **Hide** (when one is up), **Cancel**, **Show** (primary). Enter = Show; Esc closes only it. A fill error shows inline and keeps the overlay open.

- [ ] **Step 1: Write the failing tests:** `ctrl_shift_m_opens_show_message_on_last_template`, `message_shortcut_works_over_open_item_edit` (typed text in Item edit survives), `enter_in_show_message_shows_banner_and_closes`, `empty_field_keeps_overlay_open`, `hide_button_only_while_message_up`, `practice_warning_shows_in_practice` (Needs FEAT-6).
- [ ] **Step 2:** `cargo test ui::dialogs::show_message` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Ctrl+Shift+M and the Show message overlay`

### Task B11: Top bar button, icons, widgets and Help

**Files:**
- Modify: `ui/operator/topbar.slint` (`automation-btn`, icon-only below 1500 px), `ui/widgets.slint` (`TimeField`, `ColorSwatches`), `ui/icons.slint` (`clock`, `bolt`, `message`, `lock`), the Help rows (Plan 3's Help dialog: Ctrl+Shift+M "Show a message")

**Interfaces:** `TimeField` edits a `TimeOfDay` as 12-hour text and returns `TimeOfDay::parse`-compatible `"HH:MM"`; `ColorSwatches` shows the eight `MacroColor` values. `automation-btn` opens `DialogKind::Automation(AutomationTab::Timers)` (B13).

- [ ] **Step 1: Write the failing tests:** `automation_button_icon_only_below_1500`, `time_field_round_trips_12_hour`, `color_swatches_pick_macro_color`, `help_lists_message_shortcut`.
- [ ] **Step 2:** `cargo test ui::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): automation top bar button, TimeField, swatches and icons`

### Task B12: Item edit, Services start time and the slides badge

**Files:**
- Modify: `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs` (`advance-on`, `advance-secs`, `advance-loop`; disabled with `NEEDS_TWO_SLIDES` under 2 slides; for Song, Scripture, Announcement and Slides only), `ui/dialogs/services.slint`, `src/ui/dialogs/services.rs` (a Starts at `TimeField`; `RESTART_FOR_NEW_TIME` while the pre-service or Service start timer is RunningTo), `ui/operator/slides.slint`, `src/ui/operator/slides.rs` (a clock badge on the first thumbnail of an advancing item)

- [ ] **Step 1: Write the failing tests:** `item_edit_saves_auto_advance`, `item_edit_one_slide_disables_advance`, `services_starts_at_round_trips`, `services_says_restart_while_countdown_runs`, `slides_badge_on_advancing_item`.
- [ ] **Step 2:** `cargo test ui::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): auto-advance in Item edit, service start time, slides badge`

### Task B13: Automation dialog shell

**Files:**
- Create: `ui/dialogs/automation.slint` (the tab bar and an empty content area per tab), `src/ui/dialogs/automation/mod.rs`
- Modify: `src/ui/dialogs/mod.rs` (`DialogKind::Automation(AutomationTab)`; not rebuilt by `dialog_refresh` except the readouts that follow `Update::Timers`; opening and closing it sets `App.automation_dialog_open` and notifies the ticker, Plan decision 15)

**Interfaces:** `pub enum AutomationTab { Timers, Messages, Macros, PreService }`. The shell owns the shared list-and-editor layout and the delete flow: a delete returns `delete_question` text → a confirm ask → `{confirm: true}`.

- [ ] **Step 1: Write the failing tests:** `automation_dialog_opens_on_tab`, `automation_dialog_keeps_typed_text_across_data_update`, `f1_in_automation_dialog_clears_overlays`, `delete_referenced_asks_first`, `dialog_open_wakes_ticker`.
- [ ] **Step 2:** `cargo test ui::dialogs::automation` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Automation dialog shell`

### Task B14: Automation dialog — Timers tab

**Files:**
- Create: `ui/dialogs/automation-timers.slint`, `src/ui/dialogs/automation/timers.rs`

**Interfaces:** the editor and wording exactly as spec §3.2: name, kind (Countdown of a length / Countdown to a time / Count up), length or target (`TimeField`), Overrun, place (`OverlayPlace`), caption, Show on stage, Show on phone, at zero. The at-zero macro picker lists only `is_overlay_only` macros. A live readout per row follows `Update::Timers`.

- [ ] **Step 1: Write the failing tests:** `timer_editor_saves_each_kind`, `at_zero_picker_lists_only_overlay_only_macros`, `timer_row_readout_follows_update`.
- [ ] **Step 2:** `cargo test ui::dialogs::automation::timers` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Automation dialog Timers tab`

### Task B15: Automation dialog — Messages tab

**Files:**
- Create: `ui/dialogs/automation-messages.slint`, `src/ui/dialogs/automation/messages.rs`

**Interfaces:** spec §3.3: name, text with the field list under it, place (`MessagePlace`), hide after (`HIDE_AFTER_CHOICES`), Show on phone. The save shows `UNMATCHED_BRACE` as a warning, not an error.

- [ ] **Step 1: Write the failing tests:** `message_editor_shows_brace_warning`, `message_editor_lists_fields_live`, `message_editor_saves_hide_after`.
- [ ] **Step 2:** `cargo test ui::dialogs::automation::messages` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Automation dialog Messages tab`

### Task B16: Automation dialog — Macros tab

**Files:**
- Create: `ui/dialogs/automation-macros.slint`, `src/ui/dialogs/automation/macros.rs`

**Interfaces:** spec §3.4: name, colour (`ColorSwatches`), Show on phone, the step list (add, move up/down, remove). The step editor lists only `MacroStep` kinds. An Unknown step shows as "Needs a newer RustyPresenter" and can only be removed. The pre-service macro row shows a lock icon and `PRESERVICE_LOCKED`. Up/down on the list sends `automation/macro/move`.

- [ ] **Step 1: Write the failing tests:** `macro_editor_add_move_remove_steps`, `locked_preservice_macro_is_read_only`, `unknown_step_shown_and_removable`.
- [ ] **Step 2:** `cargo test ui::dialogs::automation::macros` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Automation dialog Macros tab`

### Task B17: Automation dialog — Pre-service tab

**Files:**
- Create: `ui/dialogs/automation-preservice.slint`, `src/ui/dialogs/automation/preservice.rs`

**Interfaces:** spec §3.6: the loop ComboBox lists the auto-advancing items, and the music ComboBox lists the audio items, both by title; place (default from `preservice::default_place`), caption, end choice. Save sends `automation/preservice/save`.

- [ ] **Step 1: Write the failing tests:** `preservice_card_saves_and_locks_macro`, `preservice_loop_lists_only_advancing_items`, `preservice_place_defaults_center_without_loop`.
- [ ] **Step 2:** `cargo test ui::dialogs::automation::preservice` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Automation dialog Pre-service tab`

### Task B18: Phone remote

**Files:**
- Modify: `src/server.rs` (`remote_command` allowlist exactly as spec §5.11; `RunMacro` and `ShowMessage` only when `remote::macro_allowed`/`message_allowed`, else `NOT_ALLOWED_FROM_PHONE`; relative macros need `base`, others `seen_clear`; the room `ShowMessage` uses FEAT-3's limiter, shared as `remote::RateLimit` (Needs FEAT-3; until then a local copy with the same 1/s rule), with `TOO_FAST`; `remote_view` adds `macros`, `messages` and `message_shown` (read from the frame); the stream sends `Update::Timers` only as `automation::remote::timers_message`), `web/remote.html`, `web/remote.js` (Control tab sections Message, Timers and Macros in the §5.11 order, the confirm sheet `confirm_run`, and the phone strings in one object at the top, copied from `src/strings/automation.rs`)

- [ ] **Step 1: Write the failing tests:** `remote_macro_allowed_only_when_marked`, `remote_message_allowed_only_when_marked`, `remote_timers_filtered_to_on_remote`, `remote_allowlist_matches_table` (one row per §5.11 line), `phone_show_message_rate_limited`, `phone_stream_sends_timers_only_as_timers_type`, `phone_macro_after_clear_all_is_stale`.
- [ ] **Step 2:** `cargo test server::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(remote): macros, messages and timer readout on the phone`

### Task B19: Interim stage line, manual checks and records

**Files:**
- Modify: `src/ui/stage.rs` (the first `on_stage` timer that isn't Stopped replaces the clock line, in red when overrun; a playing video or audio countdown keeps priority; **skip this sub-step if FEAT-3 has already merged**, since FEAT-3's latched timer supersedes it), `PROJECT_PLAN.md` (FEAT-2 status), `docs/superpowers/checks/` (the manual results)

- [ ] **Step 1: Write the failing test:** `stage_shows_first_running_on_stage_timer` (unless FEAT-3 is merged).
- [ ] **Step 2:** `cargo test ui::stage` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Manual checks** (spec §9) on Windows, macOS and Linux, on the reference laptop:
  - With a corner timer over a 1080p video, CPU rises ≤ 3 points over 60 s and mpv drops no extra frames over 10 min.
  - The banner text is ≥ 44 px, and a 200-letter message fits in 3 lines.
  - Close the lid for 2 min during Sermon: the readout is within 1 s of a phone stopwatch.
  - Change the time zone during a fixed-time countdown: the readout is correct within 1 s.
  - Cmd+Shift+M on macOS opens Show message and does not minimize.
  - On the phone: run a macro, show and hide a nursery message, and watch the timer line. Press F1, then tap a stale phone macro: nothing comes back.
- [ ] **Step 6:** Commit `feat(stage): interim timer line; FEAT-2 manual checks`

---

## Spec coverage

| Spec | Tasks |
|---|---|
| §4.1 forward compatibility | A1, A2, A7, A8, A9, A10, A11, B1 |
| §4.2 persisted types, seeding | A2, A7, A8, A9, A10, A11, B1 |
| §4.3 runtime | A5, A9, A10, A13, B4, B6 |
| §4.4 readouts | A3, A13, A14, B7 |
| §5.1 timer state machine | A4, A5, A6 |
| §5.2 live refactor | B2 |
| §5.3 commands, macros | A8, B3, B5 |
| §5.4 at-zero and pre-service end | A6, A8, A10, A12, B5, B6 |
| §5.5 clear matrix | B3 |
| §5.6 auto-advance | A9, A13 (`on_adopt`), B6 |
| §5.7 messages | A7, A13, B3, B5, B10 |
| §5.8 ticker and interim stage line | A13, A14, B7, B19 |
| §5.9 editing actions and validation | A6, A12, B4 |
| §5.10 restart rule | out of scope (Plan 4's recovery work follows spec §5.10) |
| §5.11 phone | A14, B18 |
| §6 rendering | B3 (`OverlayKey`), B8 |
| §7 UI | A15, B9, B10, B11, B12, B13–B17 |
| §8 errors (strings) | A4–A15, B5, B18 |
| §9 manual checks | B19 |

**Deliberate deviations from the spec's types and signatures** (all listed under Plan decisions; the spec is not edited):

| Spec | Plan | Why |
|---|---|---|
| §4.2 `TimerDef.place` / `PreService.place: Placement` (FEAT-1 object) | `OverlayPlace` choice list (string), FEAT-1 §4.5's name; mapped to `Placement` in B3 (decision 1) | FEAT-1 §4.5 says FEAT-2 keeps the small choice list; Part A doesn't depend on FEAT-1. An object stored by a newer version loads as the default (decision 18). |
| §4.2 `automation::seed(&mut Data) -> bool` | `seed(&mut Automation) -> bool` (decision 12) | Part A can't name `Data`; B1 calls it from `Data::after_load`. |
| §5.2 `apply_batch(.., steps: &[LiveStep], plan, origin) -> Outcome` | `steps: &[BatchStep]` with `BatchStep::Live { step, show }`, a runtime callback, `-> BatchOutcome` (decision 13) | Runtime steps need their own checkpoints inside the batch, and the toast needs the failed/skipped steps. |
| §4.3/§5.6 `Automation::on_adopt(item_id, map)` | `AutomationRuntime::on_adopt(item_id, &map)` (decision 14) | `Automation` is the persisted root here; the runtime holds the engines' `AutoState`. |
| §4.3 two maps `auto` / `auto_paused_item` | one `AutoState` per engine (decision 5) | Memory only. |
