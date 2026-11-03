# FEAT-6 — Service Planning: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Service dates, **Copy last week**, service templates with **blanks**, print/save of the service order and lyrics (HTML first, PDF later), and **practice mode** (a second live engine that the room screen never follows, except for clears).

**Architecture:** The plan has two parts because another session is rewriting the shared files right now (Plan 3: `src/app.rs`, `src/ui/**`, `ui/**`, `src/server.rs`, `src/bin/native.rs`, `src/video`; Plan 4: nearly everything else, including `store.rs`, platform and the Tauri/web removal). **Part A** builds every pure piece of FEAT-6 in one new module tree, `src/planning/`, that touches no existing file except one `pub mod planning;` line in `src/lib.rs`. Logic that the spec puts on existing types (`ServiceItem`, `Service`, `Settings`, `Data`, `Live`, `Command`) is written in Part A against small standalone types and one trait (`PlanItem`), and is unit-tested with a test-only item type. **Part B** wires that core into model/store/live/app/server/UI after Plans 3 and 4 have merged into `native-foundation`; it is written against Plan 3's file layout.

**Tech Stack:** Rust 2024; existing `chrono` 0.4 (`default-features = false, features = ["clock"]`; `NaiveDate`, `NaiveDateTime`, `Weekday`, `Datelike` are all available with these features), `serde`/`serde_json`, `uuid`. Part A adds **no crate**: dates serialize through a small `opt_iso_date` serde module instead of chrono's `serde` feature. Part B adds `sys-locale`, `opener` (and later `krilla` for 6b-2), pinned to exact versions at that time. Slint 1.18.1 for Part B UI.

**Spec:** `docs/superpowers/specs/2026-11-01-feat6-service-planning-design.md` (FEAT-6). Also read where a task names them: FEAT-1 §4.7 (format chain, `SUPPORTED_FORMAT`, `Data::after_load`), §5.2 (clear table), §7.2 (1366×768 budget); FEAT-2 §4 (`TimeOfDay`, `lenient_time`), §5.11 (phone allowlist); FEAT-4 §4 (`SectionKind::short`), §7 (strings convention), D16; FEAT-5 §4–5 (`SecondText`, `scripture::fetch`, offline-status rule); FEAT-7 (`report_page` is shared). Plan 3 `docs/superpowers/plans/2026-11-01-native-operator.md` for the UI layout (`Ops`, `DialogShell`, ask overlays, Services/Add dialogs, follower thread).

## Global Constraints

- **Part A touches no existing file** except one line, `pub mod planning;`, in `src/lib.rs`. No edits to `src/app.rs`, `src/live.rs`, `src/model.rs`, `src/store.rs`, `src/room.rs`, `src/slides.rs`, `src/server.rs`, `src/ui/**`, `ui/**`, `web/**`, `src/bible*` or `Cargo.toml`. Part A may *read* (call) `slides::parse_sections(&str) -> Vec<Section>` and the `Section { label, groups }` fields, and nothing else from the app — this keeps the one shared dependency as small as possible, because Plan 4 and FEAT-4 may change `slides.rs` (Task B1 re-runs the print tests after the rebase).
- **Module paths deviate from the spec on purpose** while Plans 3/4 are in flight: spec `src/planning.rs` → `src/planning/{dates,region,services,kinds,items,template,lenient}.rs`; spec `src/print/` → `src/planning/print/`; spec `src/practice.rs` pure parts → `src/planning/practice/`; spec `src/report_page.rs` → `src/planning/report_page.rs` (Task B9 reconciles it with FEAT-7's copy if FEAT-7 landed first).
- **Strings files are at their final paths now** (spec, FEAT-4 §7; same pattern as FEAT-2/FEAT-3/FEAT-5): `src/strings/planning.rs` and `src/strings/print.rs` are created in Part A and included with `#[path = "../strings/planning.rs"] pub mod strings;` in `src/planning/mod.rs` and `#[path = "../../strings/print.rs"] pub mod strings;` in `src/planning/print/mod.rs`. Part A code refers to them as `crate::planning::strings` / `crate::planning::print::strings`. The strings files import nothing from `planning` (plain `&str` consts and small `fn`s over `&str`/`usize`), so they compile unchanged in both places. Task B1 declares them in `src/strings/mod.rs` and replaces the two `#[path]` lines with `pub use crate::strings::planning as strings;` / `pub use crate::strings::print as strings;` — **no file move**, and every `planning::strings::X` call site keeps working. `src/strings/` holds no `mod.rs` in Part A (it is FEAT-4's/Part B's), so no existing file is touched.
- **Unknown enum values never fail a load** (spec forward-compatibility; same reasoning as Review Focus #1 and FEAT-2's `#[default] #[serde(other)]` rule). Persisted unit enums with a default variant put `#[default] #[serde(other)]` on it: `BlankKind::Any`, `PrintWhat::Both`. Persisted `Option<enum>` fields with no natural default (`Settings.date_order: Option<DateOrder>`, `PrintSettings.paper: Option<Paper>`) use `#[serde(default, deserialize_with = "crate::planning::lenient::opt_or_none")]` (unknown value or wrong type → `None`). `ServiceTemplate.day` uses `crate::planning::template::lenient_day` (null → `None` = any day; missing or unknown → `Some(Sunday)`). Every serde path into `planning` from outside the module is written `crate::planning::…`.
- **Test command (Part A):** PowerShell `& "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd" test --release --lib <module path>` — written below as `FEAT_CARGO test --release --lib <path>`. Every Part A task also ends with `FEAT_CARGO test --release --lib planning` green.
- **Time:** `chrono` is the only time crate (no `jiff`). Calendar dates are `chrono::NaiveDate`, serialized `"YYYY-MM-DD"`; no function in `src/planning/` reads the clock — `today`/`now` are always parameters.
- **Serde:** every new persisted field has a serde default; old JSON (no `date`, no `templates`, no `print`) must load. Optional fields are `skip_serializing_if = "Option::is_none"` except `ServiceTemplate.day`, where `null` means "Any day" and must be written.
- **Strings:** every user-visible string is a `pub const` (or a small `fn` for formatted text) in `src/strings/planning.rs` / `src/strings/print.rs`, copied verbatim from the spec. Curly quotes “ ” and the en/em dashes (– —) are literal characters, as in the spec.
- **Copy rules** are the spec's §5.6 table; the `PlanItem` trait (Task A4) is the only place item fields are touched, so Part B implements the field rules once, for `model::ServiceItem`.
- **Default labels (§5.3):** Song "Song", Scripture "Bible reading", Media "Media", Deck "Slides", Text "Announcement", Any "Something".
- **Region table (§5.2):** Mdy: US, PH, FM, MH, PW. Ymd: CA, CN, JP, KR, TW, HU, LT, MN, IR. Otherwise Dmy. Letter: US, CA, MX, PH, CL, CO, VE; else A4. No locale or no region → Ymd and Letter.
- **Practice versions:** `PRACTICE_VERSION_GAP: u64 = 1_000_000_000`.
- **Part B is blocked** until `native-foundation` contains Plans 3 and 4. Before starting any Part B task, re-read the files it names; where Plan 4 moved or removed a file (for example `web/stage.html`), apply the step to its replacement or drop it and note that in the commit.

## Review Focus

1. **A hand-edited or corrupt `date` in `data.json`** (`"date": "11/8/2026"` or `"2026-02-30"`). Expected: that service loads as undated ("No date"); the rest of the file loads. Test `opt_iso_date_bad_value_loads_as_none` in Task A1.
2. **Dates across month/year ends and two-digit years.** "Service 12/27/26" dated 2026-12-27, copied → "Service 1/3/2027" dated 2027-01-03. Test `dated_name_crosses_year_end_and_reads_two_digit_year` in Task A2.
3. **Names that contain numbers but no date** ("Easter 2026", "Service 3", "Psalm 23 Night"). Expected: never stripped; the new date is appended. Test `dated_name_keeps_year_and_number_tokens` in Task A2.
4. **A template saved for "Any day"** round-trips as `"day": null` and is not turned back into Sunday on load; an old template with no `day` key is Sunday. Test `template_any_day_round_trips_as_null` in Task A5.
5. **Song lyrics or names containing HTML or CSS** (`</style><script>`, a quote in the service name used in the `@page` footer). Expected: printed as text, never as markup; no script. Test `html_escapes_style_breakout_and_footer_quotes` in Task A9.
6. **A value from a newer build or a hand edit** (`"what": "pdf"`, `"paper": "legal"`, `"want": "poll"`, `"day": "funday"`, `"date_order": "dym"`). Expected: the field falls back (Both / None / Any / Sunday / None); `Settings` and `Data` still load. Tests `unknown_date_order_is_none` (A1), `unknown_blank_want_is_any` (A4), `template_unknown_day_is_sunday` (A5), `print_settings_unknown_what_falls_back_to_both` and `unknown_paper_is_none` (A6).

---

# PART A — Core, buildable now

### Task A1: Module root, region defaults and date formatting

**Files:**
- Modify: `src/lib.rs` (add `pub mod planning;` — the only edit to an existing file in Part A)
- Create: `src/planning/mod.rs`, `src/planning/region.rs`, `src/planning/dates.rs`, `src/planning/lenient.rs`, `src/strings/planning.rs`

**Interfaces:**
- Produces (`planning::lenient`): `pub fn opt_or_none<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error>` — deserializes a `serde_json::Value`, then `serde_json::from_value::<T>(v).ok()`; `null`, a wrong type or an unknown variant → `Ok(None)`; never returns `Err`. Used as `#[serde(default, deserialize_with = "crate::planning::lenient::opt_or_none")]`.
- Produces (`planning::region`): `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)] #[serde(rename_all = "snake_case")] pub enum DateOrder { Mdy, Dmy, Ymd }`; same derives `pub enum Paper { Letter, A4 }`; `pub fn region_of(locale: &str) -> Option<String>` (upper-case 2-letter or 3-digit region; strips `.UTF-8` and `@…`; accepts `-` and `_`); `pub fn region_defaults(locale: Option<&str>) -> (DateOrder, Paper)`; `pub fn effective(order: Option<DateOrder>, paper: Option<Paper>, locale: Option<&str>) -> (DateOrder, Paper)` (overrides win).
- Produces (`planning::dates`): `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)] #[serde(rename_all = "snake_case")] pub enum Day { Sunday, Monday, Tuesday, Wednesday, Thursday, Friday, Saturday }` with `From<Day> for chrono::Weekday` and `Day::of(d: NaiveDate) -> Day`; `pub fn format_date(d: NaiveDate, o: DateOrder) -> String` (`11/8/2026`, `8/11/2026`, `2026-11-08`; no zero padding except ISO); `pub fn format_row_date(d: Option<NaiveDate>, o: DateOrder) -> String` ("Sun 11/8/2026" or `strings::NO_DATE`); `pub fn format_date_long(d: NaiveDate, o: DateOrder) -> String` (Mdy "Sunday, November 8, 2026"; Dmy "Sunday 8 November 2026"; Ymd "Sunday, 2026-11-08" — English month/day names); `pub fn parse_iso(s: &str) -> Result<NaiveDate, &'static str>` (strict `YYYY-MM-DD`, error `strings::BAD_DATE`); `pub mod opt_iso_date { serialize / deserialize }` for `Option<NaiveDate>` — deserialize is **lenient**: missing, `null`, a non-string or an unparsable string → `None`.
- `src/planning/mod.rs` declares `pub mod region; pub mod dates; pub mod lenient;` and `#[path = "../strings/planning.rs"] pub mod strings;` (later tasks add their own lines here) and `#[cfg(test)] pub(crate) mod test_item;` from Task A4 on.
- `src/strings/planning.rs` starts with `NO_DATE = "No date"`, `BAD_DATE = "That date isn't valid."`.

- [ ] **Step 1: Write the failing tests** (in `region.rs` and `dates.rs` `#[cfg(test)] mod tests`; helper `fn d(y: i32, m: u32, day: u32) -> NaiveDate`)

```rust
#[test] fn region_defaults_us_gb_jp_unknown() {
    assert_eq!(region_defaults(Some("en-US")), (DateOrder::Mdy, Paper::Letter));
    assert_eq!(region_defaults(Some("en_GB.UTF-8")), (DateOrder::Dmy, Paper::A4));
    assert_eq!(region_defaults(Some("ja-JP")), (DateOrder::Ymd, Paper::A4));
    assert_eq!(region_defaults(Some("zh-Hans-CN")), (DateOrder::Ymd, Paper::A4));
    assert_eq!(region_defaults(Some("en-CA")), (DateOrder::Ymd, Paper::Letter));
    assert_eq!(region_defaults(Some("es-MX")), (DateOrder::Dmy, Paper::Letter));
    assert_eq!(region_defaults(Some("fr")), (DateOrder::Ymd, Paper::Letter)); // no region
    assert_eq!(region_defaults(Some("C")), (DateOrder::Ymd, Paper::Letter));
    assert_eq!(region_defaults(None), (DateOrder::Ymd, Paper::Letter));
}
#[test] fn settings_override_region() {
    assert_eq!(effective(Some(DateOrder::Dmy), None, Some("en-US")), (DateOrder::Dmy, Paper::Letter));
    assert_eq!(effective(None, Some(Paper::A4), Some("en-US")), (DateOrder::Mdy, Paper::A4));
}
#[test] fn format_date_three_orders() {
    assert_eq!(format_date(d(2026,11,8), DateOrder::Mdy), "11/8/2026");
    assert_eq!(format_date(d(2026,11,8), DateOrder::Dmy), "8/11/2026");
    assert_eq!(format_date(d(2026,11,8), DateOrder::Ymd), "2026-11-08");
    assert_eq!(format_row_date(Some(d(2026,11,8)), DateOrder::Mdy), "Sun 11/8/2026");
    assert_eq!(format_row_date(None, DateOrder::Mdy), "No date");
}
#[test] fn format_date_long_by_region() {
    assert_eq!(format_date_long(d(2026,11,8), DateOrder::Mdy), "Sunday, November 8, 2026");
    assert_eq!(format_date_long(d(2026,11,8), DateOrder::Dmy), "Sunday 8 November 2026");
    assert_eq!(format_date_long(d(2026,11,8), DateOrder::Ymd), "Sunday, 2026-11-08");
}
#[test] fn day_maps_to_weekday() { assert_eq!(Day::of(d(2026,11,8)), Day::Sunday); assert_eq!(chrono::Weekday::from(Day::Tuesday), chrono::Weekday::Tue); }
#[test] fn parse_iso_strict() { assert_eq!(parse_iso("2026-11-08"), Ok(d(2026,11,8))); assert_eq!(parse_iso("11/8/2026"), Err("That date isn't valid.")); assert!(parse_iso("2026-02-30").is_err()); }
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Holder { #[serde(default, with = "opt_iso_date", skip_serializing_if = "Option::is_none")] date: Option<NaiveDate> }
#[test] fn service_date_serializes_iso() {
    assert_eq!(serde_json::to_string(&Holder { date: Some(d(2026,11,8)) }).unwrap(), r#"{"date":"2026-11-08"}"#);
    assert_eq!(serde_json::to_string(&Holder { date: None }).unwrap(), "{}");
    assert_eq!(serde_json::from_str::<Holder>("{}").unwrap(), Holder { date: None });
}
#[test] fn opt_iso_date_bad_value_loads_as_none() {
    for j in [r#"{"date":"11/8/2026"}"#, r#"{"date":"2026-02-30"}"#, r#"{"date":null}"#, r#"{"date":5}"#] {
        assert_eq!(serde_json::from_str::<Holder>(j).unwrap(), Holder { date: None }, "{j}");
    }
}
// in region.rs tests (exercises planning::lenient::opt_or_none the way Settings.date_order uses it)
#[derive(Deserialize, PartialEq, Debug)]
struct OrderHolder { #[serde(default, deserialize_with = "crate::planning::lenient::opt_or_none")] date_order: Option<DateOrder> }
#[test] fn unknown_date_order_is_none() {
    assert_eq!(serde_json::from_str::<OrderHolder>(r#"{"date_order":"dmy"}"#).unwrap().date_order, Some(DateOrder::Dmy));
    for j in [r#"{"date_order":"dym"}"#, r#"{"date_order":3}"#, r#"{"date_order":null}"#, "{}"] {
        assert_eq!(serde_json::from_str::<OrderHolder>(j).unwrap().date_order, None, "{j}");
    }
}
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::` — Expected: FAIL to compile (`region_defaults`, `format_date`, `opt_or_none`… not found).
- [ ] **Step 3: Implement** the Interfaces above. Lenient deserialize: deserialize an `#[serde(untagged)] enum Raw { S(String), Other(serde::de::IgnoredAny) }` and parse `S` with `parse_iso`; `Other` or any parse failure → `None`.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::` — Expected: PASS (9 tests).
- [ ] **Step 5: Commit** `feat(planning): region defaults, service date formats and lenient ISO date serde`.

### Task A2: Dated names and next dates

**Files:**
- Modify: `src/planning/dates.rs`, `src/strings/planning.rs`

**Interfaces:**
- Consumes: Task A1 `DateOrder`, `Day`, `format_date`.
- Produces: `pub fn parse_date_token(tok: &str) -> Vec<NaiveDate>` (every reading of a token as M/D/Y, D/M/Y with 2- or 4-digit year — 2-digit `yy` = `2000 + yy` — or `Y-M-D`; empty when none); `pub fn strip_trailing_date(name: &str, old: Option<NaiveDate>, order: DateOrder) -> Option<String>` (the name without its last token and the separators before it — spaces, `-`, `–` — when the token reads as `old`; when `old == None`, when it reads as a valid date in `order` or ISO); `pub fn dated_name(name: &str, old: Option<NaiveDate>, new: NaiveDate, order: DateOrder) -> String` (`strip_trailing_date(..).unwrap_or(name.trim_end())` + `" "` + `format_date(new, order)`; an empty base gives just the date); `pub fn next_week_date(source: Option<NaiveDate>, today: NaiveDate) -> NaiveDate`; `pub fn default_date(day: Option<Day>, today: NaiveDate) -> NaiveDate`; `pub fn template_name_from_service(name: &str, date: Option<NaiveDate>, order: DateOrder) -> String` (`strip_trailing_date(..).unwrap_or(name)`, trimmed); `pub fn new_service_name(template_name: &str, date: NaiveDate, order: DateOrder) -> String` (`"<template name> <date>"`).
- Token rule: the token is the text after the last space or `–`; a token that itself contains a leading `-` separator (`"Service-11/8/2026"`) is split at the first `-` followed by a digit only when the remainder parses. After the token is removed, trailing spaces, `-` and `–` are trimmed, so `"Service - 2026-11-01"` and `"Service – 11/1/2026"` both leave `"Service"`. The token is first tried whole (so `"2026-11-01"` reads as ISO before any `-` split).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn dated_name_replaces_us_date() { assert_eq!(dated_name("Service 11/1/2026", Some(d(2026,11,1)), d(2026,11,8), DateOrder::Mdy), "Service 11/8/2026"); }
#[test] fn dated_name_replaces_uk_date_matching_old() { assert_eq!(dated_name("Service 8/11/2026", Some(d(2026,11,8)), d(2026,11,15), DateOrder::Dmy), "Service 15/11/2026"); }
#[test] fn dated_name_keeps_non_matching_date_text() { assert_eq!(dated_name("Service 9/9/2026", Some(d(2026,11,8)), d(2026,11,15), DateOrder::Mdy), "Service 9/9/2026 11/15/2026"); }
#[test] fn dated_name_replaces_iso_date() { assert_eq!(dated_name("Service 2026-11-08", Some(d(2026,11,8)), d(2026,11,15), DateOrder::Mdy), "Service 11/15/2026"); }
#[test] fn dated_name_appends_when_no_date() { assert_eq!(dated_name("Sunday Morning", None, d(2026,11,15), DateOrder::Dmy), "Sunday Morning 15/11/2026"); }
#[test] fn dated_name_strips_en_dash() { assert_eq!(dated_name("Service – 11/1/2026", Some(d(2026,11,1)), d(2026,11,8), DateOrder::Mdy), "Service 11/8/2026"); }
#[test] fn dated_name_old_none_strips_date_in_computer_order_or_iso() {
    assert_eq!(dated_name("Service 11/1/2026", None, d(2026,11,8), DateOrder::Mdy), "Service 11/8/2026");
    assert_eq!(dated_name("Service 2026-11-01", None, d(2026,11,8), DateOrder::Dmy), "Service 8/11/2026");
    assert_eq!(dated_name("Service 13/1/2026", None, d(2026,11,8), DateOrder::Mdy), "Service 13/1/2026 11/8/2026"); // not valid M/D/Y
}
#[test] fn dated_name_crosses_year_end_and_reads_two_digit_year() { assert_eq!(dated_name("Service 12/27/26", Some(d(2026,12,27)), d(2027,1,3), DateOrder::Mdy), "Service 1/3/2027"); }
#[test] fn dated_name_keeps_year_and_number_tokens() {
    assert_eq!(dated_name("Easter 2026", None, d(2026,4,5), DateOrder::Mdy), "Easter 2026 4/5/2026");
    assert_eq!(dated_name("Psalm 23 Night", None, d(2026,4,5), DateOrder::Mdy), "Psalm 23 Night 4/5/2026");
    assert_eq!(dated_name("Service 3", None, d(2026,4,5), DateOrder::Mdy), "Service 3 4/5/2026");
}
#[test] fn dated_name_strips_hyphen_separator() {
    assert_eq!(dated_name("Service-11/1/2026", Some(d(2026,11,1)), d(2026,11,8), DateOrder::Mdy), "Service 11/8/2026");
    assert_eq!(dated_name("Service - 2026-11-01", Some(d(2026,11,1)), d(2026,11,8), DateOrder::Mdy), "Service 11/8/2026");
    assert_eq!(dated_name("Service-11/1/2026", None, d(2026,11,8), DateOrder::Mdy), "Service 11/8/2026");
    assert_eq!(dated_name("Mid-week", None, d(2026,11,4), DateOrder::Mdy), "Mid-week 11/4/2026"); // "-week" does not parse: untouched
}
#[test] fn next_week_adds_seven_days() { assert_eq!(next_week_date(Some(d(2026,11,1)), d(2026,11,3)), d(2026,11,8)); }
#[test] fn next_week_skips_to_future_same_weekday() { assert_eq!(next_week_date(Some(d(2026,10,4)), d(2026,11,3)), d(2026,11,8)); }
#[test] fn next_week_exactly_today_is_kept() { assert_eq!(next_week_date(Some(d(2026,10,27)), d(2026,11,3)), d(2026,11,3)); }
#[test] fn next_week_undated_is_next_sunday() { assert_eq!(next_week_date(None, d(2026,11,3)), d(2026,11,8)); }
#[test] fn next_week_on_sunday_undated_is_today() { assert_eq!(next_week_date(None, d(2026,11,1)), d(2026,11,1)); }
#[test] fn default_date_next_weekday_includes_today() {
    assert_eq!(default_date(Some(Day::Sunday), d(2026,11,3)), d(2026,11,8));
    assert_eq!(default_date(Some(Day::Tuesday), d(2026,11,3)), d(2026,11,3));
}
#[test] fn default_date_any_day_is_today() { assert_eq!(default_date(None, d(2026,11,3)), d(2026,11,3)); }
#[test] fn template_and_service_names() {
    assert_eq!(template_name_from_service("Sunday Service 11/1/2026", Some(d(2026,11,1)), DateOrder::Mdy), "Sunday Service");
    assert_eq!(template_name_from_service("Sunday Service", None, DateOrder::Mdy), "Sunday Service");
    assert_eq!(new_service_name("Sunday Service", d(2026,11,8), DateOrder::Mdy), "Sunday Service 11/8/2026");
}
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::dates` — Expected: FAIL (functions missing).
- [ ] **Step 3: Implement** the Interfaces. `next_week_date`: `s + 7 days`; if that is before `today`, the first date `>= today` with `s`'s weekday; `None` → `default_date(Some(Day::Sunday), today)`.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::dates` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): dated service names, next-week and template dates`.

### Task A3: Service list order, week source and list strings

**Files:**
- Create: `src/planning/services.rs`
- Modify: `src/planning/mod.rs`, `src/strings/planning.rs`

**Interfaces:**
- Consumes: Task A1/A2 dates.
- Produces: `#[derive(Clone, Copy, Debug)] pub struct Dated { pub id: Uuid, pub date: Option<NaiveDate> }` (Part B builds a `Vec<Dated>` from `Data.services`); `pub fn week_source(services: &[Dated], current: Option<Uuid>, today: NaiveDate) -> Option<Uuid>` (latest `date <= today`, ties → later index; none → `current`); `pub fn find_on_date(services: &[Dated], date: NaiveDate) -> Option<Uuid>` (first match in stored order); `pub fn list_order(services: &[Dated]) -> Vec<usize>` (dated newest first, ties later index first; then undated in stored order); `pub const VISIBLE_SERVICES: usize = 8`; `pub fn split_older(order: &[usize], expanded: bool) -> (&[usize], usize /*hidden count*/)`.
- Strings (`src/strings/planning.rs`): `fn items_count(n) -> String` ("1 item" / "3 items"); `fn blanks_chip(n) -> String` ("1 blank" / "2 blanks"); `fn show_older(n) -> String` ("Show 1 older service" / "Show 5 older services"); `NO_SERVICE_TO_COPY = "There is no service to copy."`; `fn ready_toast(name, blanks) -> String` ("“N” is ready", "“N” is ready — fill in 1 blank", "… — fill in 4 blanks"); `fn copy_week_question(date_text, source_name) -> String` ("Make a service for Sun 11/8/2026 from “Service 11/1/2026”?" — the UI bolds the date); `fn already_dated(date_text, name) -> String` ("There is already a service for Sun 11/8/2026 (“Service 11/8/2026”)."); `LEAVE_BLANKS = "Leave blanks for songs and Bible readings"`; button texts `COPY_LAST_WEEK = "Copy last week"`, `NEW_FROM_TEMPLATE = "New from template…"`, `NEW_EMPTY = "New empty service"`, `COPY_THIS = "Copy this service"`, `OPEN_IT = "Open it"`, `MAKE_ANOTHER = "Make another"`.

- [ ] **Step 1: Write the failing tests**

```rust
// ids a, b, c, dd, e = Uuid::from_u128(1..=5) (`d` is the date helper); dt(id, date) builds a Dated; today() = 2026-11-03 (Tue)
#[test] fn week_source_prefers_latest_past_date() { let s = [dt(a, Some(d(2026,10,25))), dt(b, Some(d(2026,11,1))), dt(c, None)]; assert_eq!(week_source(&s, Some(c), today()), Some(b)); }
#[test] fn week_source_ignores_future_dated_services() { let s = [dt(a, Some(d(2026,11,1))), dt(b, Some(d(2026,12,24)))]; assert_eq!(week_source(&s, Some(b), today()), Some(a)); }
#[test] fn week_source_future_only_falls_back_to_current() { let s = [dt(a, Some(d(2026,11,8))), dt(b, Some(d(2026,12,24)))]; assert_eq!(week_source(&s, Some(b), today()), Some(b)); }
#[test] fn week_source_ties_take_later_service() { let s = [dt(a, Some(d(2026,11,1))), dt(b, Some(d(2026,11,1)))]; assert_eq!(week_source(&s, Some(a), today()), Some(b)); }
#[test] fn week_source_falls_back_to_current_when_undated() { let s = [dt(a, None), dt(b, None)]; assert_eq!(week_source(&s, Some(b), today()), Some(b)); assert_eq!(week_source(&[], None, today()), None); }
#[test] fn find_on_date_first_match() { let s = [dt(a, Some(d(2026,11,8))), dt(b, Some(d(2026,11,8)))]; assert_eq!(find_on_date(&s, d(2026,11,8)), Some(a)); assert_eq!(find_on_date(&s, d(2026,11,1)), None); }
#[test] fn services_sorted_by_date_undated_last() {
    let s = [dt(a, None), dt(b, Some(d(2026,10,25))), dt(c, Some(d(2026,11,8))), dt(e, None), dt(dd, Some(d(2026,11,1)))];
    assert_eq!(list_order(&s), vec![2, 4, 1, 0, 3]);
}
#[test] fn older_services_collapse_after_eight() {
    let order: Vec<usize> = (0..11).collect();
    assert_eq!(split_older(&order, false), (&order[..8], 3));
    assert_eq!(split_older(&order, true), (&order[..], 0));
}
#[test] fn list_strings_pluralise() {
    assert_eq!(items_count(1), "1 item"); assert_eq!(items_count(3), "3 items");
    assert_eq!(blanks_chip(1), "1 blank"); assert_eq!(blanks_chip(2), "2 blanks");
    assert_eq!(show_older(3), "Show 3 older services");
    assert_eq!(ready_toast("Service 11/8/2026", 0), "“Service 11/8/2026” is ready");
    assert_eq!(ready_toast("Service 11/8/2026", 4), "“Service 11/8/2026” is ready — fill in 4 blanks");
    assert_eq!(copy_week_question("Sun 11/8/2026", "Service 11/1/2026"), "Make a service for Sun 11/8/2026 from “Service 11/1/2026”?");
    assert_eq!(already_dated("Sun 11/8/2026", "Service 11/8/2026"), "There is already a service for Sun 11/8/2026 (“Service 11/8/2026”).");
}
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::services` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the Interfaces; add `pub mod services;` to `src/planning/mod.rs`.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::services` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): week source, date lookup and Services list order`.

### Task A4: Blank kinds, the `PlanItem` trait and copying items

**Files:**
- Create: `src/planning/kinds.rs`, `src/planning/items.rs`, `src/planning/test_item.rs` (`#[cfg(test)]` only)
- Modify: `src/planning/mod.rs`, `src/strings/planning.rs`

**Interfaces:**
- Produces (`kinds`): `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)] #[serde(rename_all = "snake_case")] pub enum BlankKind { Song, Scripture, Media, Deck, Text, #[default] #[serde(other)] Any }` (an unknown `want` from a newer build loads as `Any`); `pub fn default_label(k: BlankKind) -> &'static str` (Global Constraints values); `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)] pub struct Blank { #[serde(default)] pub label: String, #[serde(default)] pub want: BlankKind }` — the field set Part B puts in `ItemKind::Blank { label, want }`; `pub fn blank_title(label: &str) -> String` (`"<label> (blank)"`, or `"Blank"` when the label is empty — `slides::build_item`'s title); `pub fn row_text(label: &str, want: BlankKind) -> String` ("Song — click to choose"; empty label uses `default_label(want)`); `pub fn choose_button(want: BlankKind) -> &'static str` (Song "Choose a song", Scripture "Choose a Bible reading", Media "Choose media", Deck "Choose slides", Text "Write an announcement", Any "Choose what goes here"); `pub fn add_tab_for(want: BlankKind) -> BlankKind` (Any → Song); `pub fn fill_title(want) -> String` ("Fill in: Song"); `pub const USE_THIS = "Use this"`; `pub const BLANK_NOT_LIVE = "This is a blank — choose what goes here first."`; `pub const ADD_BLANK_CHOICES: [BlankKind; 6]` (Song, Scripture, Media, Deck, Text, Any — "Add a blank" menu order).
- Produces (`items`):

```rust
pub trait PlanItem: Clone {
    fn id(&self) -> Uuid;
    fn set_id(&mut self, id: Uuid);
    /// Some((label, want)) when this item is a blank.
    fn as_blank(&self) -> Option<(&str, BlankKind)>;
    /// The kind this item asks for when blanked: Song→Song … Text→Text; a blank returns its `want`.
    fn blank_kind(&self) -> BlankKind;
    /// A blank with `label` and `want = blank_kind()`; same id; keeps notes, background, theme, advance,
    /// stage_layout; drops slide_notes and pco_item_id (spec §5.6 "blanked row").
    fn to_blank(&self, label: String) -> Self;
    /// §5.6 rules shared by every copy: drop pco_item_id; drop Song.arrangement unless `keep_arrangement`;
    /// keep Scripture (and second) verse text only for offline-status translations.
    fn strip_for_copy(&mut self, keep_arrangement: bool);
    fn stage_layout(&self) -> Option<Uuid>;
    fn clear_stage_layout(&mut self);
    /// The filled item (§5.6 "Fill a blank" column) before its id is reset; use `fill_notes` and `or_blank`.
    fn fill_from_blank(blank: &Self, chosen: Self) -> Self;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum CopyMode { LastWeek { blanks: bool }, ThisService }
pub fn copy_items<I: PlanItem>(items: &[I], mode: CopyMode, layouts: &HashSet<Uuid>) -> (Vec<I>, usize /*blanks*/);
pub fn count_blanks<I: PlanItem>(items: &[I]) -> usize;
pub fn fill_notes(blank: &str, chosen: &str) -> String;           // "b\n\nc"; either alone when the other is empty
pub fn or_blank<T>(chosen: Option<T>, blank: Option<T>) -> Option<T>;
```

- `copy_items`: every item gets `Uuid::new_v4()`; `LastWeek { blanks: true }` turns Song/Scripture (by `blank_kind`, non-blank items only) into `to_blank(default_label(..))`; everything else gets `strip_for_copy(mode == ThisService)`; existing blanks stay blanks; a `stage_layout` not in `layouts` is cleared.
- `test_item.rs`: `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct TestItem { pub id: Uuid, pub kind: TKind, pub notes: String, pub background: Option<String>, pub theme: Option<String>, pub advance: Option<u32>, pub stage_layout: Option<Uuid>, pub slide_notes: Vec<String>, pub pco_item_id: Option<String> }`, `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub enum TKind { Song { arrangement: Option<Vec<String>> }, Scripture { offline: bool, verses: Vec<String> }, Media, Deck, Text, Blank(Blank) }`, and `impl PlanItem for TestItem` following the trait docs exactly (it is the reference Part B's `ServiceItem` impl copies). Fixtures (every constructor uses `Uuid::new_v4()` for `id` and the shared field values unless stated):
  - shared values: `notes: "Key of G"`, `background: Some("bg.jpg")`, `theme: Some("dark")`, `advance: Some(5)`, `stage_layout: Some(LAYOUT)` with `pub const LAYOUT: Uuid = Uuid::from_u128(100)`, `slide_notes: vec!["Welcome everyone"]`, `pco_item_id: Some("pco-1")`;
  - `song()` → `TKind::Song { arrangement: Some(vec!["V1", "C", "V2"]) }`;
  - `reading(offline: bool)` → `TKind::Scripture { offline, verses: vec!["16 For God so loved the world"] }`;
  - `media()` → `TKind::Media`; `text()` → `TKind::Text`;
  - `blank(want: BlankKind)` → `TKind::Blank(Blank { label: String::new(), want })` with `notes: "Blank note"`, `background: Some("blank-bg.jpg")`, `slide_notes: vec![]`, `pco_item_id: None`;
  - `pub fn all_layouts(items: &[TestItem]) -> HashSet<Uuid>` — every `Some(stage_layout)` in `items` (used by A4 and A5 tests as "every layout still exists").

- [ ] **Step 1: Write the failing tests** (in `items.rs`, using `TestItem`)

```rust
#[test] fn copy_items_new_ids_keep_all_fields() {
    let src = vec![song(), reading(true), media(), text()];
    let (out, blanks) = copy_items(&src, CopyMode::ThisService, &all_layouts(&src));
    assert_eq!(blanks, 0);
    for (a, b) in src.iter().zip(&out) { assert_ne!(a.id, b.id); assert_eq!((&a.notes, &a.background, &a.theme, a.advance, a.stage_layout, &a.slide_notes), (&b.notes, &b.background, &b.theme, b.advance, b.stage_layout, &b.slide_notes)); }
}
#[test] fn copy_items_blanks_keep_notes_background_advance_layout_drop_slide_notes() {
    let src = vec![song(), reading(false), media()];
    let (out, blanks) = copy_items(&src, CopyMode::LastWeek { blanks: true }, &all_layouts(&src));
    assert_eq!(blanks, 2);
    assert_eq!(out[0].as_blank(), Some(("Song", BlankKind::Song)));
    assert_eq!(out[1].as_blank(), Some(("Bible reading", BlankKind::Scripture)));
    assert!(out[2].as_blank().is_none());
    assert_eq!((&out[0].notes, &out[0].background, &out[0].theme, out[0].advance, out[0].stage_layout), (&src[0].notes, &src[0].background, &src[0].theme, src[0].advance, src[0].stage_layout));
    assert!(out[0].slide_notes.is_empty());
}
#[test] fn copy_items_keeps_existing_blanks() { let (out, n) = copy_items(&[blank(BlankKind::Media)], CopyMode::LastWeek { blanks: false }, &HashSet::new()); assert_eq!(n, 1); assert_eq!(out[0].as_blank().unwrap().1, BlankKind::Media); }
#[test] fn copy_last_week_drops_arrangement() { let (out, _) = copy_items(&[song()], CopyMode::LastWeek { blanks: false }, &HashSet::new()); assert!(matches!(&out[0].kind, TKind::Song { arrangement: None })); }
#[test] fn copy_this_service_keeps_arrangement() { let (out, _) = copy_items(&[song()], CopyMode::ThisService, &HashSet::new()); assert!(matches!(&out[0].kind, TKind::Song { arrangement: Some(_) })); }
#[test] fn copies_drop_pco_item_id() { for m in [CopyMode::ThisService, CopyMode::LastWeek { blanks: false }, CopyMode::LastWeek { blanks: true }] { let (out, _) = copy_items(&[song(), media()], m, &HashSet::new()); assert!(out.iter().all(|i| i.pco_item_id.is_none())); } }
#[test] fn copies_keep_item_theme() { let (out, _) = copy_items(&[media()], CopyMode::LastWeek { blanks: false }, &HashSet::new()); assert_eq!(out[0].theme, media().theme); }
#[test] fn copy_gone_stage_layout_becomes_none() { let (out, _) = copy_items(&[media()], CopyMode::ThisService, &HashSet::new()); assert_eq!(out[0].stage_layout, None); }
#[test] fn online_scripture_copy_has_no_verse_text() { let (out, _) = copy_items(&[reading(false)], CopyMode::ThisService, &HashSet::new()); assert!(matches!(&out[0].kind, TKind::Scripture { verses, .. } if verses.is_empty())); }
#[test] fn offline_scripture_copy_keeps_verse_text() { let (out, _) = copy_items(&[reading(true)], CopyMode::ThisService, &HashSet::new()); assert!(matches!(&out[0].kind, TKind::Scripture { verses, .. } if !verses.is_empty())); }
#[test] fn fill_notes_joins_with_blank_line() { assert_eq!(fill_notes("Key of G", "Skip v3"), "Key of G\n\nSkip v3"); assert_eq!(fill_notes("", "x"), "x"); assert_eq!(fill_notes("x", ""), "x"); }
#[test] fn blank_texts() { assert_eq!(blank_title("Song"), "Song (blank)"); assert_eq!(blank_title(""), "Blank"); assert_eq!(row_text("", BlankKind::Scripture), "Bible reading — click to choose"); assert_eq!(choose_button(BlankKind::Any), "Choose what goes here"); assert_eq!(add_tab_for(BlankKind::Any), BlankKind::Song); assert_eq!(fill_title(BlankKind::Song), "Fill in: Song"); }
#[test] fn blank_item_round_trips() { let b = Blank { label: "Offering song".into(), want: BlankKind::Song }; let j = serde_json::to_string(&b).unwrap(); assert_eq!(serde_json::from_str::<Blank>(&j).unwrap(), b); assert_eq!(serde_json::from_str::<Blank>("{}").unwrap(), Blank { label: String::new(), want: BlankKind::Any }); }
#[test] fn unknown_blank_want_is_any() { assert_eq!(serde_json::from_str::<Blank>(r#"{"label":"Poll","want":"poll"}"#).unwrap(), Blank { label: "Poll".into(), want: BlankKind::Any }); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::items` — Expected: FAIL (modules missing).
- [ ] **Step 3: Implement** `kinds.rs`, `items.rs` and `test_item.rs`; add `pub mod kinds; pub mod items;` and `#[cfg(test)] pub(crate) mod test_item;` to `mod.rs`; strings into `src/strings/planning.rs` and used from `kinds.rs` as `super::strings::…`.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::items` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): blank kinds, PlanItem trait and copy rules`.

### Task A5: Templates, service from template and filling a blank

**Files:**
- Create: `src/planning/template.rs`
- Modify: `src/planning/mod.rs`, `src/strings/planning.rs`

**Interfaces:**
- Consumes: Task A4 `PlanItem`, `BlankKind`, `default_label`; Task A2 `Day`, `default_date`.
- Produces:

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ServiceTemplate<I> {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "sunday", deserialize_with = "lenient_day")] pub day: Option<Day>,  // None = any day; written as null, never skipped
    pub items: Vec<I>,
    // Part B Task B1 adds `theme: Option<ThemeId>` and `starts_at: Option<TimeOfDay>` (FEAT-1/FEAT-2 types).
}
pub fn sunday() -> Option<Day>;
/// null → None (any day); a known day name → Some(day); anything else (unknown name, number) → Some(Sunday).
/// A missing key never reaches this function: `default = "sunday"` covers it.
pub fn lenient_day<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Day>, D::Error>;
pub fn default_keep<I: PlanItem>(item: &I) -> bool;   // false for blanks, Song and Scripture; true otherwise
pub fn template_items<I: PlanItem>(items: &[I], keep: &HashSet<Uuid>, labels: &HashMap<Uuid, String>) -> Vec<I>;
pub fn items_from_template<I: PlanItem>(items: &[I], layouts: &HashSet<Uuid>) -> (Vec<I>, usize);
pub fn validate_template(name: &str, parts: usize) -> Result<(), &'static str>;
pub fn fill<I: PlanItem>(items: &mut [I], blank_id: Uuid, chosen: I) -> Result<Uuid, &'static str>;
pub fn template_summary(day: Option<Day>, parts: usize, blanks: usize) -> (String /*"Sundays"/"Any day"*/, String /*"5 parts, 2 blanks"*/);
```

- `template_items`: fresh ids; an item in `keep` that is not a blank → `strip_for_copy(false)`; otherwise `to_blank(label)` where label = `labels[id]` (trimmed, non-empty), else the blank's own non-empty label, else `default_label(blank_kind())`. Template layout refs are kept as they are (FEAT-3 §5.6).
- `items_from_template`: fresh ids; `strip_for_copy(false)`; gone `stage_layout` → cleared; returns the blank count. (Items only. The spec's `service_from_template` also sets `template = Some(t.id)`, theme and `starts_at` from the template; those are `Service` fields, so Part B Task B4 sets them and tests them as `service_from_template_fresh_ids_and_template_id`.)
- `fill`: finds `blank_id` → else `Err(strings::BLANK_GONE)`; not a blank → `Err(strings::NOT_A_BLANK)`; replaces in place with `I::fill_from_blank(&blank, chosen)` and then `set_id(blank_id)`; returns `blank_id`.
- Strings: `BLANK_GONE = "That blank is gone."`, `NOT_A_BLANK = "That item is not a blank any more."`, `TEMPLATE_GONE = "That template is gone."`, `TEMPLATE_NEEDS_NAME = "A template needs a name."`, `TEMPLATE_NEEDS_PART = "A template needs at least one part."`, `NO_TEMPLATES = "No templates yet. Use “Save as template” on a service to make one."`, `NEED_TEMPLATE_TIP = "Save a service as a template first"`, `fn template_saved(name)` ("Template “Sunday Service” saved"), `fn delete_template_q(name)` ("Delete the template “N”? (Services made from it stay.)"), `RENAME_TEMPLATE = "Rename template"`, `KEEP = "Keep"`, `BLANK = "Blank"`, `ADD_A_BLANK = "Add a blank"`, `SAVE_AS_TEMPLATE = "Save as template"`, `MAKE_SERVICE = "Make service"`, day names plural ("Sundays" … "Saturdays") and `ANY_DAY = "Any day"`, `fn in_template(name)` ("This is in the template “N”. Remove it from the template first.").

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn template_from_service_keep_and_blank() {
    let src = vec![media(), song(), text()];
    let keep: HashSet<Uuid> = [src[0].id, src[2].id].into();
    let t = template_items(&src, &keep, &HashMap::from([(src[1].id, "Opening song".to_string())]));
    assert!(t.iter().zip(&src).all(|(a, b)| a.id != b.id));
    assert!(t[0].as_blank().is_none() && t[2].as_blank().is_none());
    assert_eq!(t[1].as_blank(), Some(("Opening song", BlankKind::Song)));
    assert!(t[1].slide_notes.is_empty() && t[1].notes == src[1].notes);
}
#[test] fn template_blank_labels_default_by_kind() { let src = vec![song(), reading(true), media(), text()]; let t = template_items(&src, &HashSet::new(), &HashMap::new()); let labels: Vec<_> = t.iter().map(|i| i.as_blank().unwrap().0.to_string()).collect(); assert_eq!(labels, ["Song", "Bible reading", "Media", "Announcement"]); }
#[test] fn template_drops_arrangement_and_online_verse_text() { let src = vec![song(), reading(false)]; let keep: HashSet<Uuid> = src.iter().map(|i| i.id).collect(); let t = template_items(&src, &keep, &HashMap::new()); assert!(matches!(&t[0].kind, TKind::Song { arrangement: None })); assert!(matches!(&t[1].kind, TKind::Scripture { verses, .. } if verses.is_empty())); }
#[test] fn default_keep_songs_and_readings_are_blank() { assert!(!default_keep(&song())); assert!(!default_keep(&reading(true))); assert!(!default_keep(&blank(BlankKind::Any))); assert!(default_keep(&media())); }
#[test] fn items_from_template_fresh_ids_and_blank_count() { let t = vec![media(), blank(BlankKind::Song), blank(BlankKind::Scripture)]; let (out, n) = items_from_template(&t, &all_layouts(&t)); assert_eq!(n, 2); assert!(out.iter().zip(&t).all(|(a, b)| a.id != b.id)); }
#[test] fn service_from_template_gone_layout_is_none() { let (out, _) = items_from_template(&[media()], &HashSet::new()); assert_eq!(out[0].stage_layout, None); }
#[test] fn fill_replaces_blank_in_place_keeps_id_and_notes() {
    let mut items = vec![media(), blank(BlankKind::Song)]; let id = items[1].id; let blank_notes = items[1].notes.clone();
    let chosen = TestItem { background: None, ..song() };
    assert_eq!(fill(&mut items, id, chosen.clone()), Ok(id));
    assert_eq!(items[1].id, id); assert!(items[1].as_blank().is_none());
    assert_eq!(items[1].notes, fill_notes(&blank_notes, &chosen.notes));
}
#[test] fn fill_keeps_blank_background_when_choice_has_none() { let mut items = vec![blank(BlankKind::Song)]; let bg = items[0].background.clone(); let id = items[0].id; fill(&mut items, id, TestItem { background: None, ..song() }).unwrap(); assert_eq!(items[0].background, bg); }
#[test] fn fill_refuses_non_blank() { let mut items = vec![media()]; let id = items[0].id; assert_eq!(fill(&mut items, id, song()), Err("That item is not a blank any more.")); }
#[test] fn fill_missing_blank_is_gone() { let mut items = vec![media()]; assert_eq!(fill(&mut items, Uuid::new_v4(), song()), Err("That blank is gone.")); }
#[test] fn validate_template_rules() { assert_eq!(validate_template("  ", 1), Err("A template needs a name.")); assert_eq!(validate_template("Sunday", 0), Err("A template needs at least one part.")); assert_eq!(validate_template("Sunday", 1), Ok(())); }
#[test] fn template_any_day_round_trips_as_null() {
    let t = ServiceTemplate::<TestItem> { id: Uuid::nil(), name: "Midweek".into(), day: None, items: vec![] };
    let j = serde_json::to_value(&t).unwrap(); assert!(j["day"].is_null());
    assert_eq!(serde_json::from_value::<ServiceTemplate<TestItem>>(j).unwrap().day, None);
    let old = serde_json::json!({"id": Uuid::nil(), "name": "Old", "items": []});
    assert_eq!(serde_json::from_value::<ServiceTemplate<TestItem>>(old).unwrap().day, Some(Day::Sunday));
}
#[test] fn template_unknown_day_is_sunday() {
    for day in [serde_json::json!("funday"), serde_json::json!(3)] {
        let j = serde_json::json!({"id": Uuid::nil(), "name": "New", "day": day, "items": []});
        assert_eq!(serde_json::from_value::<ServiceTemplate<TestItem>>(j).unwrap().day, Some(Day::Sunday));
    }
    let j = serde_json::json!({"id": Uuid::nil(), "name": "Tue", "day": "tuesday", "items": []});
    assert_eq!(serde_json::from_value::<ServiceTemplate<TestItem>>(j).unwrap().day, Some(Day::Tuesday));
}
#[test] fn template_summary_text() { assert_eq!(template_summary(Some(Day::Sunday), 8, 2), ("Sundays".into(), "8 parts, 2 blanks".into())); assert_eq!(template_summary(None, 1, 1), ("Any day".into(), "1 part, 1 blank".into())); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::template` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the Interfaces; `pub mod template;` in `mod.rs`.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::template` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): service templates, template-made services and blank filling`.

### Task A6: Print settings, request and service order blocks

**Files:**
- Create: `src/planning/print/mod.rs` (with `#[path = "../../strings/print.rs"] pub mod strings;`), `src/planning/print/doc.rs`, `src/strings/print.rs`, `src/planning/print/fixtures.rs` (`#[cfg(test)]` only)
- Modify: `src/planning/mod.rs` (`pub mod print;`)

**Interfaces:**
- Consumes: Task A1 `DateOrder`, `Paper`, `format_date_long`, `format_date`; Task A4 `BlankKind`, `default_label`.
- Produces (`print/mod.rs`): `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)] #[serde(rename_all = "snake_case")] pub enum PrintWhat { Order, Lyrics, #[default] #[serde(other)] Both }`; `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] #[serde(default)] pub struct PrintSettings { pub what: PrintWhat, pub notes: bool, pub scripture: bool, pub song_per_page: bool, #[serde(deserialize_with = "crate::planning::lenient::opt_or_none", skip_serializing_if = "Option::is_none")] pub paper: Option<Paper> }` with `Default` = Both / true / true / false / None; `#[derive(Clone, Copy, Debug, PartialEq)] pub struct PrintRequest { pub what: PrintWhat, pub notes: bool, pub scripture: bool, pub song_per_page: bool, pub paper: Paper }`.
- Input types (Part B fills them from `Data`; pure here). Derives: `#[derive(Clone, Debug, PartialEq)]` on `PrintHeader`, `PrintItem`, `PrintKind`, `PrintSong`, `PrintReading`, `VerseText`; `#[derive(Clone, Copy, Debug, PartialEq, Eq)]` on `MediaWhat` (tests call `.clone()` and compare): `pub struct PrintHeader { pub service_name: String, pub date: Option<NaiveDate>, pub order: DateOrder, pub church_ccli: String, pub printed_on: NaiveDate }`; `pub struct PrintItem { pub kind: PrintKind, pub notes: String }`; `pub enum PrintKind { Song(PrintSong), Scripture(PrintReading), Media { name: String, what: MediaWhat, looped: bool }, Deck { title: String, slides: usize }, Text { title: String, body: String }, Blank { label: String, want: BlankKind } }`; `pub enum MediaWhat { Video, Picture, Audio }`; `pub struct PrintSong { pub title: String, pub writers: String, pub copyright: String, pub ccli: String, pub lyrics: String, pub order: Vec<String> /*labels: item override, else song's; empty = written order*/, pub order_short: Vec<String> /*FEAT-4 short names for `order`, Part B fills*/ }`; `pub struct PrintReading { pub reference: String, pub code: String, pub verses: VerseText, pub second: Option<(String /*code*/, VerseText)>, pub copyright: Option<String> }`; `pub enum VerseText { Verses(Vec<(String /*verse number*/, String)>), Failed }`.
- Produces (`doc.rs`): `pub struct PrintDoc { pub title: String, pub subtitle: String, pub footer: String, pub paper: Paper, pub blocks: Vec<Block> }`; `pub enum Block { Heading(String), Credit(String), OrderRow { n: usize, kind: String, title: String, detail: String, notes: String, blank: bool }, Order(String), Section { label: String, lines: Vec<String> }, Para(String), PageBreak }` (derive `Clone, Debug, PartialEq`); `pub fn header(h: &PrintHeader, paper: Paper) -> PrintDoc` (title = name; subtitle = `format_date_long` or ""; footer = "Printed from RustyPresenter · <format_date(printed_on)>"; `Credit("CCLI License # 123456")` first when `church_ccli` is non-empty); `pub fn order_blocks(items: &[PrintItem], notes: bool) -> Vec<Block>`.
- Order rows (spec §6.2), kind / title / detail: Song "Song" / title / writers, then `" · Order: V1 C V2 C"` from `order_short` when non-empty; Scripture "Bible reading" / reference / code, `" + <second code>"`; Media "Media" / name / "Video"|"Picture"|"Audio" + " (loops)"; Deck "Slides" / title / "1 slide"|"N slides"; Text "Announcement" / title / first non-empty body line, cut to 79 chars + "…" when longer than 80; Blank "Blank" / "Blank — <label or default_label>" / "(not chosen yet)", `blank: true`. `notes` field is the item's notes only when `notes == true`. Numbering `n` starts at 1 and counts every item.

- `fixtures.rs` (`#[cfg(test)] pub(crate) mod fixtures;` in `print/mod.rs`; used by A6, A7 and A9 tests), exact values:
  - `pub fn d(y, m, day) -> NaiveDate`; `pub const LYRICS: &str = "Verse 1\nAmazing grace\nhow sweet\n\nthe sound\nChorus\nMy chains are gone\nVerse 2\nT'was grace";`
  - `empty_song() -> PrintSong`: every `String` empty, both `Vec`s empty.
  - `p_song() -> PrintItem`: `notes: ""`, `PrintKind::Song(PrintSong { title: "Amazing Grace", writers: "John Newton", copyright: "Public Domain", ccli: "22025", lyrics: LYRICS, order: ["Verse 1", "Chorus", "Verse 2"], order_short: ["V1", "C", "V2"] })`.
  - `song_with(lyrics: &str, order: &[&str]) -> PrintItem`: `p_song()` with that `lyrics`, `order` = the given labels and `order_short` empty.
  - `p_reading_two()`: `notes: ""`, `PrintKind::Scripture(PrintReading { reference: "John 3:16-17", code: "KJV", verses: Verses([("16", "For God so loved the world"), ("17", "For God sent not")]), second: Some(("NIV", Verses([("16", "For God so loved"), ("17", "For God did not send")]))), copyright: None })`.
  - `p_reading_failed()`: `PrintReading { reference: "Romans 8:28", code: "ESV", verses: Failed, second: None, copyright: None }`.
  - `p_reading_with_copyright(c: &str)`: `p_reading_two()` with `second: None` and `copyright: Some(c)`.
  - `p_video_looped()`: `PrintKind::Media { name: "Walk-in loop", what: MediaWhat::Video, looped: true }`.
  - `p_deck(n: usize)`: `PrintKind::Deck { title: "Sermon", slides: n }`.
  - `p_text(body: &str)`: `PrintKind::Text { title: "Notices", body }`.
  - `p_blank(label: &str, want: BlankKind)`: `PrintKind::Blank { label, want }`.
  - `hdr() -> PrintHeader`: `{ service_name: "Sunday Service", date: Some(d(2026,11,8)), order: DateOrder::Mdy, church_ccli: "", printed_on: d(2026,11,6) }`.
  - `req(what: PrintWhat) -> PrintRequest`: `{ what, notes: false, scripture: true, song_per_page: false, paper: Paper::Letter }`.
  - (A9 adds) `doc_with(blocks: Vec<Block>) -> PrintDoc`: `{ title: "Sunday Service", subtitle: "", footer: "Printed from RustyPresenter · 11/6/2026", paper: Paper::Letter, blocks }`; `doc_paper(p: Paper)`: `doc_with(vec![])` with `paper: p`; `full_doc()`: `document(&hdr(), &[p_song(), p_reading_two(), p_video_looped(), p_deck(12), p_text("Welcome!"), p_blank("", BlankKind::Song)], &PrintRequest { notes: true, ..req(PrintWhat::Both) })`.
  - Every item fixture has `notes: ""` unless a test overrides it.

- [ ] **Step 1: Write the failing tests** (in `doc.rs`, `use super::fixtures::*;`)

```rust
#[test] fn print_settings_default() { assert_eq!(serde_json::from_str::<PrintSettings>("{}").unwrap(), PrintSettings { what: PrintWhat::Both, notes: true, scripture: true, song_per_page: false, paper: None }); }
#[test] fn print_settings_partial_json_keeps_defaults() { let p: PrintSettings = serde_json::from_str(r#"{"what":"order","paper":"a4"}"#).unwrap(); assert_eq!((p.what, p.notes, p.paper), (PrintWhat::Order, true, Some(Paper::A4))); }
#[test] fn print_settings_unknown_what_falls_back_to_both() { let p: PrintSettings = serde_json::from_str(r#"{"what":"pdf","notes":false}"#).unwrap(); assert_eq!((p.what, p.notes), (PrintWhat::Both, false)); }
#[test] fn unknown_paper_is_none() { for j in [r#"{"paper":"legal"}"#, r#"{"paper":7}"#, r#"{"paper":null}"#] { assert_eq!(serde_json::from_str::<PrintSettings>(j).unwrap().paper, None, "{j}"); } assert_eq!(serde_json::to_string(&PrintSettings::default()).unwrap().contains("paper"), false); }
#[test] fn order_rows_for_every_kind() {
    let rows = order_blocks(&[p_song(), p_reading_two(), p_video_looped(), p_deck(12), p_text("Welcome!\nCoffee after"), p_blank("", BlankKind::Song)], false);
    let got: Vec<_> = rows.iter().map(|b| match b { Block::OrderRow { n, kind, title, detail, blank, .. } => (*n, kind.as_str(), title.as_str(), detail.as_str(), *blank), _ => panic!() }).collect();
    assert_eq!(got, vec![
        (1, "Song", "Amazing Grace", "John Newton · Order: V1 C V2", false),
        (2, "Bible reading", "John 3:16-17", "KJV + NIV", false),
        (3, "Media", "Walk-in loop", "Video (loops)", false),
        (4, "Slides", "Sermon", "12 slides", false),
        (5, "Announcement", "Notices", "Welcome!", false),
        (6, "Blank", "Blank — Song", "(not chosen yet)", true)]);
}
#[test] fn order_uses_item_arrangement_override() { let mut s = p_song(); if let PrintKind::Song(x) = &mut s.kind { x.order_short = vec!["C".into()]; } assert!(matches!(&order_blocks(&[s], false)[0], Block::OrderRow { detail, .. } if detail.ends_with("Order: C"))); }
#[test] fn order_marks_blanks() { assert!(matches!(&order_blocks(&[p_blank("Offering song", BlankKind::Song)], false)[0], Block::OrderRow { title, blank: true, .. } if title == "Blank — Offering song")); }
#[test] fn order_includes_notes_only_when_asked() { let i = PrintItem { notes: "Key of G".into(), ..p_song() }; assert!(matches!(&order_blocks(&[i.clone()], false)[0], Block::OrderRow { notes, .. } if notes.is_empty())); assert!(matches!(&order_blocks(&[i], true)[0], Block::OrderRow { notes, .. } if notes == "Key of G")); }
#[test] fn order_text_detail_cut_at_80() { let long = "x".repeat(100); assert!(matches!(&order_blocks(&[p_text(&long)], false)[0], Block::OrderRow { detail, .. } if detail.chars().count() == 80 && detail.ends_with('…'))); }
#[test] fn header_has_long_date_ccli_and_footer() {
    let doc = header(&PrintHeader { service_name: "Sunday Service".into(), date: Some(d(2026,11,8)), order: DateOrder::Mdy, church_ccli: "123456".into(), printed_on: d(2026,11,6) }, Paper::Letter);
    assert_eq!((doc.title.as_str(), doc.subtitle.as_str(), doc.footer.as_str()), ("Sunday Service", "Sunday, November 8, 2026", "Printed from RustyPresenter · 11/6/2026"));
    assert_eq!(doc.blocks, vec![Block::Credit("CCLI License # 123456".into())]);
}
```

(`order_never_prints_slide_notes` is structural — `PrintItem` has no slide-notes field — and is asserted in Part B Task B9 against the real item.)

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the Interfaces and `fixtures.rs`; every user-visible literal above lives in `src/strings/print.rs`.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): print settings, request types and service order blocks`.

### Task A7: Lyrics blocks and the whole print document

**Files:**
- Modify: `src/planning/print/doc.rs`, `src/strings/print.rs`, `src/planning/print/mod.rs`

**Interfaces:**
- Consumes: Task A6 types and fixtures; `crate::slides::parse_sections(&str) -> Vec<Section>` and only `Section.label` / `Section.groups` (read-only use; nothing else from `slides`, so a FEAT-4 or Plan 4 change to `slides.rs` touches at most this one call — Task B1 re-runs these tests after the rebase).
- Produces: `pub fn lyrics_blocks(items: &[PrintItem], scripture: bool, song_per_page: bool) -> Vec<Block>`; `pub fn document(h: &PrintHeader, items: &[PrintItem], req: &PrintRequest) -> PrintDoc` (header, then Order → `order_blocks(items, req.notes)`; Lyrics → `lyrics_blocks`; Both → order, `PageBreak`, lyrics); `pub fn credit_lines(s: &PrintSong) -> Vec<String>` (writers; `"© <copyright>"` unless it already starts with "©"; `"CCLI Song # <ccli>"`; empties skipped).
- Per song (spec §6.3): `Heading(title)`, `Credit` lines, `Order(labels joined " · ")` where labels = `order` (case-insensitive matches to section labels only) or, when empty or nothing matches, the written order; then each **distinct** section once in first-appearance order of that list, as `Section { label, lines }` with groups joined and one `""` line between groups. Lyrics with no sections → `Heading(title)`, credits and `Para("(no lyrics)")`.
- Per reading when `scripture`: `Heading("<reference> (<code>)")`; `Para` of `"<n> <text>"` joined with " "; `VerseText::Failed` → `Para("(couldn't get the text — check the internet)")`; second → `Heading(second code)` + its `Para`; `Credit(copyright)` when `Some`.
- `song_per_page` → `PageBreak` before each song after the first. Blanks, media, decks and texts are skipped. Nothing printed → `[Para("No songs or Bible readings in this service.")]`.

- [ ] **Step 1: Write the failing tests**

```rust
// use super::fixtures::*;  (LYRICS, song_with, empty_song, p_* , hdr, req — exact values in Task A6)
#[test] fn lyrics_print_each_section_once_with_order_line() {
    let b = lyrics_blocks(&[song_with(LYRICS, &["Verse 1", "Chorus", "Verse 2", "Chorus"])], true, false);
    assert_eq!(b[0], Block::Heading("Amazing Grace".into()));
    assert!(b.contains(&Block::Order("Verse 1 · Chorus · Verse 2 · Chorus".into())));
    let secs: Vec<_> = b.iter().filter_map(|x| match x { Block::Section { label, .. } => Some(label.as_str()), _ => None }).collect();
    assert_eq!(secs, ["Verse 1", "Chorus", "Verse 2"]);
    assert!(b.contains(&Block::Section { label: "Verse 1".into(), lines: vec!["Amazing grace".into(), "how sweet".into(), "".into(), "the sound".into()] }));
}
#[test] fn lyrics_unknown_order_labels_fall_back_to_written_order() { let b = lyrics_blocks(&[song_with(LYRICS, &["Tag"])], true, false); assert!(b.contains(&Block::Order("Verse 1 · Chorus · Verse 2".into()))); }
#[test] fn lyrics_credit_lines() { assert_eq!(credit_lines(&PrintSong { writers: "John Newton".into(), copyright: "Public Domain".into(), ccli: "22025".into(), ..empty_song() }), ["John Newton", "© Public Domain", "CCLI Song # 22025"]); }
#[test] fn lyrics_no_lyrics_prints_title() { assert!(lyrics_blocks(&[song_with("", &[])], true, false).contains(&Block::Para("(no lyrics)".into()))); }
#[test] fn lyrics_print_second_translation() { let b = lyrics_blocks(&[p_reading_two()], true, false); assert!(b.contains(&Block::Heading("John 3:16-17 (KJV)".into()))); assert!(b.contains(&Block::Heading("NIV".into()))); assert!(b.contains(&Block::Para("16 For God so loved the world 17 For God sent not".into()))); }
#[test] fn lyrics_fetch_failure_prints_reference_only() { let b = lyrics_blocks(&[p_reading_failed()], true, false); assert!(b.contains(&Block::Para("(couldn't get the text — check the internet)".into()))); }
#[test] fn lyrics_copyright_from_bible_sources() { let b = lyrics_blocks(&[p_reading_with_copyright("NIV® © Biblica")], true, false); assert!(b.contains(&Block::Credit("NIV® © Biblica".into()))); }
#[test] fn lyrics_skip_blanks() { assert_eq!(lyrics_blocks(&[p_blank("", BlankKind::Song)], true, false), vec![Block::Para("No songs or Bible readings in this service.".into())]); }
#[test] fn lyrics_scripture_optional() { assert!(!lyrics_blocks(&[p_reading_two()], false, false).iter().any(|b| matches!(b, Block::Heading(_)))); }
#[test] fn lyrics_song_per_page_inserts_breaks() { let b = lyrics_blocks(&[p_song(), p_song(), p_song()], true, true); assert_eq!(b.iter().filter(|x| **x == Block::PageBreak).count(), 2); assert_ne!(b[0], Block::PageBreak); }
#[test] fn lyrics_empty_service_message() { assert_eq!(lyrics_blocks(&[], true, false), vec![Block::Para("No songs or Bible readings in this service.".into())]); }
#[test] fn both_is_order_pagebreak_lyrics() { let doc = document(&hdr(), &[p_song()], &req(PrintWhat::Both)); let i = doc.blocks.iter().position(|b| *b == Block::PageBreak).unwrap(); assert!(matches!(doc.blocks[i - 1], Block::OrderRow { .. })); assert!(matches!(doc.blocks[i + 1], Block::Heading(_))); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: FAIL (`lyrics_blocks` missing).
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): lyric sheet blocks and full print document`.

### Task A8: Print dialog strings and file names

**Files:**
- Modify: `src/planning/print/mod.rs`, `src/strings/print.rs`

**Interfaces:**
- Produces: `pub fn save_file_name(service_name: &str, what: PrintWhat, ext: &str) -> String` ("<name> – order and lyrics.html", "– order", "– lyrics"; characters `\ / : * ? " < > |` and control characters become `-`; leading/trailing dots and spaces trimmed; empty name → "Service"); `pub fn report_stem(service_name: &str) -> String` (same sanitising, spaces → `-`, at most 60 chars).
- Strings (verbatim, spec §3.6 and §8): `PRINT_TITLE = "Print or save"`, `WHAT_ORDER = "Service order"`, `WHAT_LYRICS = "Lyrics"`, `WHAT_BOTH = "Both"`, `OPT_NOTES = "Include item notes"`, `OPT_SCRIPTURE = "Include Bible readings with the lyrics"`, `OPT_SONG_PAGE = "Start each song on a new page"`, `PAPER = "Paper:"`, `PAPER_LETTER = "Letter"`, `PAPER_A4 = "A4"`, `BTN_PRINT = "Print…"`, `BTN_SAVE_HTML = "Save web page…"`, `BTN_SAVE_PDF = "Save PDF…"`, `BTN_CLOSE = "Close"`, `OPENED_TOAST = "Opened in your web browser — use Print there (you can also save as PDF from the print window)"`, `fn saved_toast(file)` ("Saved <file>"), `fn page_failed(reason)` ("Couldn't make the page: <reason>."), `fn not_opened(path)` ("The page is saved at <path> but no program opened it. Open it from there to print."), `fn save_failed(err)` ("Couldn't save to that folder: <err>."), `fn pdf_failed(reason)` ("Couldn't make the PDF: <reason>. Try “Save web page…” instead.").

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn save_file_names() {
    assert_eq!(save_file_name("Service 11/8/2026", PrintWhat::Both, "html"), "Service 11-8-2026 – order and lyrics.html");
    assert_eq!(save_file_name("Sunday", PrintWhat::Order, "pdf"), "Sunday – order.pdf");
    assert_eq!(save_file_name("Sunday", PrintWhat::Lyrics, "html"), "Sunday – lyrics.html");
    assert_eq!(save_file_name(" .. ", PrintWhat::Lyrics, "html"), "Service – lyrics.html");
}
#[test] fn report_stem_is_file_safe() { assert_eq!(report_stem("Service 11/8/2026"), "Service-11-8-2026"); assert!(report_stem(&"a".repeat(200)).chars().count() <= 60); }
#[test] fn print_messages() { assert_eq!(page_failed("disk full"), "Couldn't make the page: disk full."); assert_eq!(pdf_failed("no font"), "Couldn't make the PDF: no font. Try “Save web page…” instead."); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): print dialog strings and safe file names`.

### Task A9: HTML writer

**Files:**
- Create: `src/planning/print/html.rs`
- Modify: `src/planning/print/mod.rs` (`pub mod html; pub fn html(doc: &PrintDoc) -> String`), `src/planning/print/fixtures.rs` (add `doc_with`, `doc_paper`, `full_doc` as listed in Task A6)

**Interfaces:**
- Consumes: Task A6/A7 `PrintDoc`, `Block`, `Paper`.
- Produces: `pub fn render(doc: &PrintDoc) -> String`; `pub fn escape(s: &str) -> String` (`& < > " '`); `pub fn css_string(s: &str) -> String` (a double-quoted CSS string with `\` → `\\`, `"` → `\"`, newlines → `\A `, `<` → `\3C `).
- Output (spec §6.4): `<!doctype html>`, `<meta charset="utf-8">`, `<title>` = escaped title, one inline `<style>`: `@page { size: letter|A4; margin: .75in; @bottom-left { content: <css_string(footer)> } @bottom-right { content: "Page " counter(page) " of " counter(pages) } }`, `body { font-family: "Segoe UI", "Noto Sans", sans-serif; color: #000; background: #fff }`, `.pb { break-before: page }`, `section { break-inside: avoid }`, `.blank { font-style: italic; color: #666 }`, `.notes { font-style: italic }`. Blocks: `Heading` → `<h2>`, `Credit` → `<p class="credit">`, `OrderRow` → one `<tr>` in a `<table class="order">` (consecutive rows share one table; notes in a second `<tr class="notes">`), `Order` → `<p class="order">`, `Section` → `<section><h3>label</h3><p>` lines joined by `<br>` `</p></section>`, `Para` → `<p>`, `PageBreak` → `<div class="pb"></div>`. No `<script>`, no `http`/`https`/`//` URL anywhere, no `<link>`, no `@import`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn html_escapes_text() { let h = render(&doc_with(vec![Block::Para("<b>Tom & Jerry's \"song\"</b>".into())])); assert!(h.contains("&lt;b&gt;Tom &amp; Jerry&#39;s &quot;song&quot;&lt;/b&gt;")); }
#[test] fn html_escapes_style_breakout_and_footer_quotes() {
    let mut d = doc_with(vec![Block::Section { label: "</style><script>x()</script>".into(), lines: vec!["</style>".into()] }]);
    d.footer = "Printed from \"RP\" </style>".into();
    let h = render(&d);
    assert_eq!(h.matches("</style>").count(), 1);
    assert!(!h.to_lowercase().contains("<script"));
}
#[test] fn html_has_no_script_or_remote_urls() { let h = render(&full_doc()); let l = h.to_lowercase(); for bad in ["<script", "http:", "https:", "src=", "<link", "@import", "url("] { assert!(!l.contains(bad), "{bad}"); } }
#[test] fn html_page_size_from_paper() { assert!(render(&doc_paper(Paper::Letter)).contains("size: letter")); assert!(render(&doc_paper(Paper::A4)).contains("size: A4")); }
#[test] fn html_page_break_and_blank_row() { let h = render(&doc_with(vec![Block::PageBreak, Block::OrderRow { n: 1, kind: "Blank".into(), title: "Blank — Song".into(), detail: "(not chosen yet)".into(), notes: String::new(), blank: true }])); assert!(h.contains(r#"<div class="pb"></div>"#)); assert!(h.contains(r#"class="blank""#)); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::print::html` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::print` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): self-contained HTML print page`.

### Task A10: Report page files (write and 30-day cleanup)

**Files:**
- Create: `src/planning/report_page.rs`
- Modify: `src/planning/mod.rs` (`pub mod report_page;`)

**Interfaces:**
- Produces: `pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600)`; `pub fn write_report(dir: &Path, stem: &str, html: &str, stamp: NaiveDateTime) -> io::Result<PathBuf>` (creates `dir`; file `<stem>-<YYYYMMDD-HHMMSS>.html`; if it exists, `-2`, `-3`…; writes atomically: `<name>.tmp` + `fs::rename`); `pub fn clean_old_reports(dir: &Path, now: SystemTime, max_age: Duration) -> usize` (deletes only `*.html` files directly in `dir` whose modified time is older than `max_age`; missing dir → 0; returns the count).
- Part B wraps these as the spec's `report_page::write_and_open(store, stem, html)` (`dir = <data>/reports`, `stamp = Local::now()`, then `opener::open`). FEAT-7 shares this helper; whichever lands first builds it (see Task B9).

- [ ] **Step 1: Write the failing tests** (helpers: `fn tmp() -> PathBuf` = `std::env::temp_dir().join(format!("rp-report-{}", Uuid::new_v4()))`, created with `create_dir_all`; `fn dt(y, mo, d, h, mi, s) -> NaiveDateTime` via `NaiveDate::from_ymd_opt(..).unwrap().and_hms_opt(..).unwrap()`)

```rust
#[test] fn report_page_writes_to_reports_with_stamp() { let dir = tmp().join("reports"); let p = write_report(&dir, "Service-11-8-2026", "<p>x</p>", dt(2026,11,6,9,30,5)).unwrap(); assert_eq!(p, dir.join("Service-11-8-2026-20261106-093005.html")); assert_eq!(std::fs::read_to_string(&p).unwrap(), "<p>x</p>"); }
#[test] fn report_page_same_second_gets_suffix() { let dir = tmp(); let a = write_report(&dir, "s", "1", dt(2026,11,6,9,30,5)).unwrap(); let b = write_report(&dir, "s", "2", dt(2026,11,6,9,30,5)).unwrap(); assert_ne!(a, b); assert!(b.ends_with("s-20261106-093005-2.html")); }
#[test] fn report_page_cleans_old() {
    let dir = tmp(); let old = dir.join("old.html"); let new = dir.join("new.html"); let other = dir.join("keep.txt");
    for p in [&old, &new, &other] { std::fs::write(p, "x").unwrap(); }
    let now = SystemTime::now();
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(now - Duration::from_secs(31 * 86400)).unwrap();
    std::fs::File::options().write(true).open(&other).unwrap().set_modified(now - Duration::from_secs(31 * 86400)).unwrap();
    assert_eq!(clean_old_reports(&dir, now, MAX_AGE), 1);
    assert!(!old.exists() && new.exists() && other.exists());
}
#[test] fn clean_missing_dir_is_zero() { assert_eq!(clean_old_reports(&tmp().join("nope"), SystemTime::now(), MAX_AGE), 0); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::report_page` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::report_page` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): report page writer with 30-day cleanup`.

### Task A11: Practice versions, ids and the command routing table

**Files:**
- Create: `src/planning/practice/mod.rs`, `src/planning/practice/route.rs`
- Modify: `src/planning/mod.rs` (`pub mod practice;`), `src/strings/planning.rs`

**Interfaces:**
- Produces (`practice/mod.rs`): `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")] pub enum EngineId { Real, Practice }`; `pub const PRACTICE_VERSION_GAP: u64 = 1_000_000_000`; `pub fn start_version(real_version: u64, floor: u64) -> u64` (`max(real + GAP, floor)`); `pub fn floor_after(practice_version: u64) -> u64` (`+ 1`); `pub fn practice_id(id: &str) -> String` (`"<id>#practice"`); `pub fn logo_id(u: Uuid) -> String` (`"practice-logo-<u>"`); `pub fn take_id(u: Uuid) -> String` (`"practice-take-<u>"`); `pub fn room_id(u: Uuid) -> String` (`"room-<u>"`); `pub fn take_target(cursor: Option<(Uuid, usize)>, service_items: &[Uuid]) -> Option<(Uuid, usize)>`.
- Produces (`route.rs`), the spec §5.5 table as data:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmdClass {
    Navigate,        // v1 Show/Next/Prev/GoToCue/TakeLive, FEAT-4 Jump/Refresh
    OverlayOn,       // FEAT-1 Overlay{on:true}
    MessageShow, TimerShow, AutoOn,            // FEAT-2
    Clear,           // ClearAll/Slide/Media/Audio, Logo, FEAT-1 ClearOverlays
    OffOrHide,       // Overlay{on:false}, HideMessage, timer hide, auto/set{on:false}
    StageState,      // FEAT-3 stage/message/*, stage/layout/*
    Macro,           // FEAT-2 RunMacro
    ScreenView,      // FEAT-7 SetScreenView
    Pause,           // MediaPause, AudioPause
    Internal(EngineId), // FEAT-2 AutoNext/AutoHideMessage of a run on that engine
    TimerAtZero,
    RoomButton,      // practice/room
    RoomEvent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route { Practice, Both, Real, Stage, PerStep, Engine(EngineId), EnginesShowingTimer, NotInPractice }
pub fn route(c: CmdClass) -> Route;          // only consulted while practice is on
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum StepRoute { Both, Practice, Stage, Refuse }
pub fn step_route(c: CmdClass) -> StepRoute;
```

- `step_route` (a macro step while practice is on) is an exhaustive `match` with **no `_` arm**, one arm per class:
  - `Clear`, `OffOrHide` → `Both` (a clear always reaches the room);
  - `StageState` → `Stage` (FEAT-3 `stage/message/*`, `stage/layout/*` steps go to stage state, unchanged by practice — spec §5.5; FEAT-3's `LayoutRef::MacroStep` shows macros carry them);
  - `Navigate`, `OverlayOn`, `MessageShow`, `TimerShow`, `AutoOn`, `Macro` → `Practice`;
  - `ScreenView`, `RoomButton`, `RoomEvent` → `Refuse` (room-only);
  - `Pause` → `Refuse` (same as `route(Pause) == NotInPractice`);
  - `Internal(_)`, `TimerAtZero` → `Refuse` (never macro steps; refusing is the safe answer if one ever appears).

- Strings: `PRACTICE_ON = "Practice is already on."`, `PRACTICE_OFF = "Practice is not on."`, `PRACTICE_ENDED = "Practice has ended."`, `NOT_PLAYED = "Not played in practice."`, `SLIDE_GONE = "That slide isn't in the service any more."`, `fn step_not_in_practice(macro_name, n)` ("<macro>: step N — not available in practice.").

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn practice_versions_start_above_gap() { assert_eq!(start_version(7, 0), 1_000_000_007); assert_eq!(start_version(7, 2_000_000_000), 2_000_000_000); }
#[test] fn second_practice_session_starts_above_first() { let s1 = start_version(5, 0); let floor = floor_after(s1 + 40); let s2 = start_version(9, floor); assert!(s2 > s1 + 40); }
#[test] fn gap_is_safe_in_javascript() { assert!(PRACTICE_VERSION_GAP < (1u64 << 53) / 1000); }
#[test] fn practice_ids() { let u = Uuid::nil(); assert_eq!(practice_id("abc"), "abc#practice"); assert_eq!(logo_id(u), format!("practice-logo-{u}")); assert_eq!(take_id(u), format!("practice-take-{u}")); assert_eq!(room_id(u), format!("room-{u}")); }
#[test] fn take_target_needs_item_in_service() { let a = Uuid::from_u128(1); assert_eq!(take_target(Some((a, 2)), &[a]), Some((a, 2))); assert_eq!(take_target(Some((a, 2)), &[]), None); assert_eq!(take_target(None, &[a]), None); }
#[test] fn routing_table_matches_spec() {
    use CmdClass::*;
    let want = [(Navigate, Route::Practice), (OverlayOn, Route::Practice), (MessageShow, Route::Practice), (TimerShow, Route::Practice), (AutoOn, Route::Practice),
        (Clear, Route::Both), (OffOrHide, Route::Both), (StageState, Route::Stage), (Macro, Route::PerStep), (ScreenView, Route::Real),
        (Pause, Route::NotInPractice), (Internal(EngineId::Real), Route::Engine(EngineId::Real)), (Internal(EngineId::Practice), Route::Engine(EngineId::Practice)),
        (TimerAtZero, Route::EnginesShowingTimer), (RoomButton, Route::Real), (RoomEvent, Route::Real)];
    for (c, r) in want { assert_eq!(route(c), r, "{c:?}"); }
}
#[test] fn macro_steps_clear_reaches_room_and_room_only_steps_refused() {
    assert_eq!(step_route(CmdClass::Clear), StepRoute::Both); assert_eq!(step_route(CmdClass::OffOrHide), StepRoute::Both);
    assert_eq!(step_route(CmdClass::ScreenView), StepRoute::Refuse); assert_eq!(step_route(CmdClass::Navigate), StepRoute::Practice);
    assert_eq!(step_route(CmdClass::RoomButton), StepRoute::Refuse); assert_eq!(step_route(CmdClass::RoomEvent), StepRoute::Refuse);
    assert_eq!(step_route(CmdClass::StageState), StepRoute::Stage);
    assert_eq!(step_route(CmdClass::Pause), StepRoute::Refuse);
    assert_eq!(step_route(CmdClass::MessageShow), StepRoute::Practice); assert_eq!(step_route(CmdClass::OverlayOn), StepRoute::Practice);
    assert_eq!(step_not_in_practice("Walk-in", 3), "Walk-in: step 3 — not available in practice.");
}
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::practice` — Expected: FAIL.
- [ ] **Step 3: Implement** (exhaustive `match` in `route` and `step_route`, no `_` arm, so a new class forces a decision).
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::practice` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): practice versions, command ids and routing table`.

### Task A12: Practice texts — room summary, start prompt, strip

**Files:**
- Create: `src/planning/practice/text.rs`
- Modify: `src/planning/practice/mod.rs`, `src/strings/planning.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomLook {
    pub connected: bool,
    pub title: Option<String>,   // live item title when a slide is up
    pub slide: Option<usize>,    // 0-based slide index when a slide is up
    pub media: Option<RoomMedia>,
    pub background: bool,        // an item background (not the default) is up
    pub overlays: usize,         // FEAT-1 overlays (0 before FEAT-1)
    pub logo: bool,
    pub music: bool,             // audio layer present and not paused
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum RoomMedia { Video, Picture }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomRunning { pub loop_: bool, pub countdown: bool, pub music: bool, pub video_next: bool, pub timer_overlay: bool, pub message_overlay: bool }
pub fn showing_something(r: &RoomLook) -> bool;           // slide || media || background || overlays > 0
pub fn room_summary(r: &RoomLook) -> String;              // text after "Room shows: "
pub fn strip_text(r: &RoomLook) -> String;                // "Practice mode — the room screen is not changing. Room shows: <summary>."
pub fn showing_sentence(r: &RoomLook) -> String;
pub fn running_lines(run: &RoomRunning) -> Vec<&'static str>;
pub fn primary_button(has_logo: bool) -> &'static str;
pub fn start_notes(run: &RoomRunning) -> Vec<&'static str>;
```

- Sources (filled in Part B Tasks B13 and B14, shown by B17; listed here so the fields mean one thing): `RoomLook.overlays = frame.overlays.len()` of the real committed frame (FEAT-1 `Frame.overlays: Vec<OverlaySlot>`; 0 before FEAT-1); `RoomRunning.timer_overlay` / `message_overlay` = the real frame has an `OverlaySlot` whose `source` is `OverlaySource::Timer { .. }` / `OverlaySource::Message { .. }` (FEAT-2 adds those variants to FEAT-1's enum); `countdown` = FEAT-2's `TimersView.timers` has a running timer; `loop_` = FEAT-2 `AutomationRuntime.pre.is_some()` (pre-service loop running); `music` = real frame audio layer present and not paused; `video_next` = real frame media is a video whose FEAT-2 `EndAction` is `Next`.
- `room_summary`: not connected → "nothing (no room screen)"; else parts in order — title+slide → "“T” (slide n+1)", media → "Video"/"Picture", background (and no title/media) → "Background", overlays → "1 overlay"/"N overlays", logo → "Logo", music → "Music playing" — joined " · "; no parts → "nothing".
- `showing_sentence`: title+slide → "The room screen is showing “Amazing Grace” (slide 3). It will stay like that while you practice."; otherwise "The room screen is showing <summary>. It will stay like that while you practice."
- `running_lines` (in this order): "Announcement loop", "Countdown", "Music", "Video — at the end it will stop instead of going to the next item".
- `primary_button`: "Show the logo, then practice" / "Clear the screen, then practice"; constants `LEAVE_IT = "Leave it and practice"`, `CANCEL = "Cancel"`, `STILL_RUNNING = "Still running in the room:"`, `PRACTICE_TIP = "Run the service here without changing the room screen"`, `ROOM_LOGO = "Room: Logo"`, `ROOM_CLEAR = "Room: Clear all"`, `ROOM_MENU = "Room ▾"`, `END = "End practice"`, `END_SHOW = "End practice and show this"`, `HEADER = "PRACTICE"`, `PHONE_BANNER = "Practice — the room screen is not changing"`.
- `start_notes` (order): timer overlay → "This also hides the countdown."; message overlay → "The message stays on the room screen."; music → "Music keeps playing.".

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn summary_and_sentence_for_slide() { let r = RoomLook { connected: true, title: Some("Amazing Grace".into()), slide: Some(2), ..Default::default() }; assert_eq!(room_summary(&r), "“Amazing Grace” (slide 3)"); assert_eq!(showing_sentence(&r), "The room screen is showing “Amazing Grace” (slide 3). It will stay like that while you practice."); assert!(showing_something(&r)); }
#[test] fn strip_text_logo_and_music() { let r = RoomLook { connected: true, logo: true, music: true, ..Default::default() }; assert_eq!(strip_text(&r), "Practice mode — the room screen is not changing. Room shows: Logo · Music playing."); assert!(!showing_something(&r)); }
#[test] fn summary_no_room_window() { assert_eq!(room_summary(&RoomLook::default()), "nothing (no room screen)"); }
#[test] fn summary_nothing() { assert_eq!(room_summary(&RoomLook { connected: true, ..Default::default() }), "nothing"); }
#[test] fn overlays_count_as_showing() { assert!(showing_something(&RoomLook { connected: true, overlays: 1, ..Default::default() })); }
#[test] fn practice_prompt_lists_running_things() { let run = RoomRunning { loop_: true, countdown: true, music: true, video_next: true, ..Default::default() }; assert_eq!(running_lines(&run), ["Announcement loop", "Countdown", "Music", "Video — at the end it will stop instead of going to the next item"]); }
#[test] fn practice_prompt_says_clear_when_no_logo() { assert_eq!(primary_button(true), "Show the logo, then practice"); assert_eq!(primary_button(false), "Clear the screen, then practice"); }
#[test] fn practice_prompt_notes_timer_and_message() { let run = RoomRunning { timer_overlay: true, message_overlay: true, music: true, ..Default::default() }; assert_eq!(start_notes(&run), ["This also hides the countdown.", "The message stays on the room screen.", "Music keeps playing."]); assert!(start_notes(&RoomRunning::default()).is_empty()); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::practice::text` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning::practice` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(planning): practice room summary, start prompt and strip texts`.

### Task A13: Forgotten-practice guard

**Files:**
- Create: `src/planning/practice/guard.rs`
- Modify: `src/planning/practice/mod.rs`, `src/strings/planning.rs`

**Interfaces:**
- Produces: `pub const START_WARN: chrono::TimeDelta` (15 min), `IDLE_WARN` (60 min), `SNOOZE` (30 min) — built with `TimeDelta::minutes` in a `const` context (chrono 0.4.45 in `Cargo.lock` supports it; otherwise use `fn`s); `#[derive(Clone, Debug, Default, PartialEq)] pub struct Guard { pub snooze_until: Option<NaiveDateTime> }`; `impl Guard { pub fn should_warn(&self, now: NaiveDateTime, starts_at: Option<NaiveDateTime>, idle: std::time::Duration) -> bool; pub fn keep(&mut self, now: NaiveDateTime, starts_at: Option<NaiveDateTime>) }`. `now`, `starts_at` and `snooze_until` are local wall-clock (`starts_at` = service date + FEAT-2 `starts_at`, built in Part B). **`idle` is a monotonic duration**: Part B keeps `Practice.last_command: Instant` (spec §5.5) and passes `last_command.elapsed()`; no `Instant` ↔ wall-clock conversion exists anywhere (Task B11 and B18 state the same).
- Rules: warn when `starts_at.is_some_and(|s| now >= s - 15 min)` or `idle >= 60 min`, unless `snooze_until.is_some_and(|u| now < u)`. `keep` sets `snooze_until = now + 30 min`, shortened to `s` when a start time `s` lies in `(now, now + 30 min)` — so the start-time warning returns at the start time itself.
- Strings: `WARN = "Practice is still on — the room screen is not changing."`, `KEEP_PRACTISING = "Keep practising"`.

- [ ] **Step 1: Write the failing tests** (`t(h, m)` = 2026-11-08 at h:m; `mins(n)` = `Duration::from_secs(n * 60)`)

```rust
#[test] fn warns_fifteen_minutes_before_start() { let g = Guard::default(); assert!(!g.should_warn(t(10,14), Some(t(10,30)), mins(4))); assert!(g.should_warn(t(10,15), Some(t(10,30)), mins(5))); }
#[test] fn practice_warning_after_idle_hour() { let g = Guard::default(); assert!(!g.should_warn(t(9,59), None, mins(59))); assert!(g.should_warn(t(10,0), None, mins(60))); }
#[test] fn keep_practising_snoozes_thirty_minutes() { let mut g = Guard::default(); g.keep(t(10,0), None); assert!(!g.should_warn(t(10,29), None, mins(149))); assert!(g.should_warn(t(10,30), None, mins(150))); }
#[test] fn snooze_ends_at_start_time() { let mut g = Guard::default(); g.keep(t(10,20), Some(t(10,30))); assert!(!g.should_warn(t(10,29), Some(t(10,30)), mins(9))); assert!(g.should_warn(t(10,30), Some(t(10,30)), mins(10))); }
#[test] fn guard_strings() { assert_eq!(WARN, "Practice is still on — the room screen is not changing."); }
```

- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::practice::guard` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release --lib planning` — Expected: PASS (all Part A tests).
- [ ] **Step 5: Commit** `feat(planning): forgotten-practice guard`.

---

# PART B — Integration after Plan 3 and Plan 4 merge

> **BLOCKED** until `native-foundation` contains Plan 3 (native operator) and Plan 4 (imports, recovery, packaging; Tauri/web removal). Start each task by rebasing this branch on `native-foundation` and re-reading the files it names. FEAT-1's format chain (`SUPPORTED_FORMAT`, `Data::after_load`) is needed for B2; when FEAT-1 has not merged yet, B2 waits and B1, B3–B10 proceed with format unchanged (the 2 → 3 step then lands with FEAT-1). B7 needs FEAT-3's `src/stage/refs.rs`; B14 needs FEAT-2's `AutoState`. Tests use the existing `cargo test` setup of those files (`test_app()`, `Ops::inline`, `tests/ui_operator_flows.rs`).
>
> **Test command:** `FEAT_CARGO test --release <target> -- <filter> [<filter>…]`. cargo takes only **one** positional test name, so every command with more than one filter puts them after `--`, where libtest takes a list (for example `FEAT_CARGO test --release --lib -- model:: slides::`). Each task ends with `FEAT_CARGO test --release` (whole suite) green.

### Task B1: Model fields, `ItemKind::Blank` and `PlanItem` for `ServiceItem`

**Files:**
- Modify: `src/model.rs`, `src/slides.rs` (`build_item` blank arm), `src/planning/template.rs` (theme/starts_at fields), `src/strings/mod.rs` (add `pub mod planning; pub mod print;`; create it per FEAT-4 §7 if absent, and add `pub mod strings;` to `src/lib.rs` if no earlier FEAT did), `src/planning/mod.rs` and `src/planning/print/mod.rs` (replace the `#[path]` lines with `pub use crate::strings::planning as strings;` and `pub use crate::strings::print as strings;` — **required**, no file move; the strings files already live at `src/strings/planning.rs` and `src/strings/print.rs`)

**Interfaces:**
- `ItemKind::Blank { #[serde(default)] label: String, #[serde(default)] want: crate::planning::kinds::BlankKind }` (`BlankKind` already falls back to `Any` on an unknown value).
- `Service { …, #[serde(default, with = "crate::planning::dates::opt_iso_date", skip_serializing_if = "Option::is_none")] date: Option<NaiveDate>, #[serde(default, skip_serializing_if = "Option::is_none")] template: Option<Uuid> }`; `Service::new(name: &str, date: Option<NaiveDate>)` (update every caller; no clock inside).
- `pub type ServiceTemplate = crate::planning::template::ServiceTemplate<ServiceItem>`; add to the generic struct `#[serde(default, skip_serializing_if = "Option::is_none")] theme: Option<theme::ThemeId>` (FEAT-1) and `#[serde(default, deserialize_with = "crate::timers::lenient_time", skip_serializing_if = "Option::is_none")] starts_at: Option<TimeOfDay>` (FEAT-2); if either FEAT has not merged, leave that field out and add it in that FEAT's merge.
- `Settings { …, #[serde(default)] print: crate::planning::print::PrintSettings, #[serde(default, deserialize_with = "crate::planning::lenient::opt_or_none", skip_serializing_if = "Option::is_none")] date_order: Option<crate::planning::region::DateOrder> }`; `Data { …, #[serde(default)] templates: Vec<ServiceTemplate> }`. Every serde path into `planning` is written `crate::planning::…` (it must resolve from `src/model.rs`).
- `impl crate::planning::items::PlanItem for ServiceItem` — follows `TestItem` exactly: arrangement = FEAT-4 `ItemKind::Song.arrangement`; verse text rule = FEAT-5 offline status (stored text exists only for offline translations, so "keep stored text" is already correct; clear `second.verses` when its translation is not offline per FEAT-5's `after_load` rule); theme/advance/stage_layout/slide_notes/pco_item_id per the FEAT-1/2/3/7 fields that exist at merge time.
- `slides::build_item` blank → `(crate::planning::kinds::blank_title(label), "blank", vec![])`.

- [ ] **Step 1: Write the failing tests** (`src/model.rs`): `old_data_json_loads_with_no_templates_and_no_dates` (today's `data.json` fixture: `templates.is_empty()`, every `date == None`, `settings.print == PrintSettings::default()`, `settings.date_order == None`), `blank_item_round_trips` (as a `ServiceItem` with `"kind":"blank"`), `service_date_serializes_iso`, `print_settings_default`, `settings_unknown_print_and_date_order_values_load` (`{"print":{"what":"pdf","paper":"legal"},"date_order":"dym"}` → `Settings` loads with `Both`, `None`, `None`), `template_theme_and_starts_at_round_trip`, `copies_drop_pco_item_id` (real `ServiceItem`; `planning_center` is a `Service` field and is tested in B4), `copy_last_week_drops_arrangement` (real `ServiceItem`), `online_scripture_copy_has_no_verse_text` (real item), and in `src/slides.rs` `build_item_blank_has_no_slides`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- model:: slides::` — Expected: FAIL (`ItemKind::Blank`, `Service.date`, `Settings.print` not found).
- [ ] **Step 3: Implement** the Interfaces; update `Service::new` callers (`Data::default()` passes `None`).
- [ ] **Step 4: Re-check the shared `slides` dependency.** Run `FEAT_CARGO test --release --lib planning::print`. Expected: PASS. If FEAT-4 or Plan 4 changed `slides::parse_sections` or `Section` (`label`/`groups`), adapt only the one call in `src/planning/print/doc.rs` until these tests pass again, and say so in the commit message.
- [ ] **Step 5: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 6: Commit** `feat(model): service dates, blanks, templates and print settings`.

### Task B2: Store — format 3, referenced assets, delete guards, first-run date

**Files:**
- Modify: `src/store.rs`, `src/model.rs` (`referenced_assets`), `src/app.rs` (delete guards; `Store::load` caller sets first service date)

**Interfaces:**
- `SUPPORTED_FORMAT = 3`; add the 2 → 3 step to FEAT-1's chain (no data change; takes the chain's "before-update" backup). `Data::default()` uses `SUPPORTED_FORMAT`.
- `Store::load` on a missing file: `Data::default()` then first service `date = Some(Local::now().date_naive())` (the only clock read).
- `referenced_assets` also walks `templates[*].items[*].background` (and FEAT-1 theme assets of templates).
- `song/delete`, `media/delete`, `deck/delete`: after the existing service check, if any template references it → `strings::planning::in_template(name)`.
- Startup: `crate::planning::report_page::clean_old_reports(<data>/reports, SystemTime::now(), MAX_AGE)` (or `crate::report_page::…` after B9) in the same place the store cleans `tmp`.

- [ ] **Step 1: Write the failing tests:** `data_default_is_supported_format`, `migrate_2_to_3_takes_before_update_backup`, `format_4_opens_read_only_and_file_untouched`, `referenced_assets_include_template_backgrounds`, `media_delete_blocked_by_template`, `song_delete_blocked_by_template`, `deck_delete_blocked_by_template`, `first_run_service_is_dated_today`, `startup_cleans_old_reports`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- store:: app::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(store): data format 3, template assets and delete guards`.

### Task B3: Blanks in live and on the phone

**Files:**
- Modify: `src/live.rs`, `src/server.rs` (`remote_view`), `web/remote.js` / phone page as left by Plan 4

**Interfaces:**
- `Live::apply` `Show` on an item with `LiveItem.kind == "blank"` → `Outcome` error `crate::planning::kinds::BLANK_NOT_LIVE` (instead of "That item has no slides."). Next/Prev already skip zero-slide items; `next_preview` uses the same rule.
- `remote_view` items gain `"blank": true` and title `"<label> (blank)"`; the phone draws them greyed and not tappable.

- [ ] **Step 1: Write the failing tests:** `next_skips_blank_items`, `prev_skips_blank_items`, `next_preview_skips_blanks`, `show_blank_says_choose_first`, `remote_view_marks_blanks`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- live:: server::` — Expected: FAIL (`show_blank_says_choose_first`, `remote_view_marks_blanks`; the skip tests may already pass — keep them as guards).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(live): blanks are skipped and cannot be shown`.

### Task B4: Planning actions in the core

**Files:**
- Modify: `src/app.rs` (`do_action` arms), `Cargo.toml` (`sys-locale`, exact version ≥ two weeks old, MIT OR Apache-2.0; `cargo about` licence list refreshed)

**Interfaces (spec §5.1, §5.3, §5.6):**
- `service/copy-week {source?, date?, blanks}` → `{id, name, date, blanks}`: source = given or `services::week_source(dated, current, today)`; date = given or `dates::next_week_date`; name = `dates::dated_name(src.name, src.date, date, order)`; items = `items::copy_items(.., CopyMode::LastWeek{blanks}, &layout_ids)`; copies `template`, FEAT-1 theme, FEAT-2 `starts_at`; drops FEAT-7 `planning_center` (when FEAT-7 is in); appends and makes current. Error `NO_SERVICE_TO_COPY`.
- `service/from-template {template, name?, date?}` → `{id, name, date, blanks}`: items = `template::items_from_template(&t.items, &layout_ids)`; **the new service gets `template = Some(t.id)`, `theme = t.theme.clone()` (FEAT-1) and `starts_at = t.starts_at` (FEAT-2)**; date default `default_date(t.day, today)`; name default `new_service_name(t.name, date, order)`; appended and made current. Error `TEMPLATE_GONE`.
- `service/set-date {id, date: String|null}` → `{}` (`parse_iso`; errors "That service is gone.", `BAD_DATE`).
- `service/fill {id, item}` → `{id}` via `template::fill` on the current service (errors `BLANK_GONE`, `NOT_A_BLANK`). Never touches the live engine.
- `template/save` (insert/replace by id; `validate_template`), `template/rename`, `template/delete`.
- `service/new` dates the new service today; with `copy_from` uses `CopyMode::ThisService` and copies `template`, theme, `starts_at`, and **drops FEAT-7 `planning_center`** (when FEAT-7 is in; spec §5.1).
- `settings/save` is FEAT-1's merge-by-key action: a request without a `print` key keeps the stored `Settings.print`; one with only `print` (or only `date_order`) changes only that key. FEAT-6 adds no new settings action.
- `App::date_order()` / `App::paper()` = `region::effective(settings.date_order, settings.print.paper, sys_locale::get_locale().as_deref())`, locale read once at startup.
- `today` = `Local::now().date_naive()` read in `do_action` only.

- [ ] **Step 1: Write the failing tests** (`src/app.rs`): `copy_week_action_selects_new_service_and_reports_blanks`, `copy_week_copies_starts_at_and_theme`, `copy_week_with_no_services_errors`, `copy_week_and_copy_this_drop_planning_center` (FEAT-7 field; written when FEAT-7 is in, else listed as pending in the commit message), `from_template_action_uses_template_day`, `service_from_template_fresh_ids_and_template_id` (spec §9.1 name: every item id differs from the template's and `service.template == Some(t.id)`), `from_template_sets_template_id_theme_and_starts_at`, `set_date_rejects_bad_date`, `fill_action_replaces_blank_and_keeps_live_unchanged`, `fill_refuses_non_blank` (action level), `template_save_requires_name_and_part`, `service_new_is_dated_today_and_copy_keeps_arrangement`, `settings_save_without_print_keeps_print` (store a non-default `print`, then `settings/save` with other keys and no `print` → stored `print` unchanged), `settings_save_print_only_changes_print` (`settings/save { print }` → `print` replaced, every other setting unchanged), `settings_save_date_order_only_changes_date_order`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib app::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): copy last week, templates, service dates and blank filling`.

### Task B5: Services dialog, Copy week, date picker and the Date format setting (UI)

**Files:**
- Modify: `ui/dialogs/services.slint`, `src/ui/dialogs/services.rs` (Plan 3 Task 19), `ui/dialogs/settings.slint` and `src/ui/dialogs/settings.rs` (Plan 3's Settings dialog, General tab), `src/strings/planning.rs` (Date format strings)
- Create: `ui/dialogs/copy-week.slint`, `src/ui/dialogs/copy_week.rs`
- Modify: `ui/icons.slint` (`template`, `print`, `calendar`, `blank`)

**Interfaces:**
- Services rows ordered by `services::list_order`, first `VISIBLE_SERVICES` then "Show N older services"; row date `format_row_date(date, App::date_order())`; chips `items_count`, `blanks_chip` (`warn`); actions Open / Rename / Date… / Print… / Save as template / trash. Top row: Copy last week (primary, disabled with no services), New from template…, New empty service, Copy this service. Body scrolls, button row fixed (fits 1366×768).
- `CopyWeek { in property <string> date-text; in property <string> source-name; in property <string> existing-name; in-out property <bool> blanks; callback copy(); callback open-existing(); callback make-another(); callback change-date(); callback cancel(); }` as an ask-level overlay (`OpState.asks`); `find_on_date` decides whether the "already" variant shows. Date… uses Slint's `DatePickerPopup` in the dialog layer with "Clear date".
- Toast after copy: `ready_toast(name, blanks)`.
- **Date format setting (Decision 10, §5.2):** a "Date format" `ComboBox` on the Settings dialog's General tab with four choices — `DATE_FORMAT_AUTO = "Automatic (from this computer)"`, `DATE_FORMAT_US = "US — 11/8/2026"`, `DATE_FORMAT_UK = "UK — 8/11/2026"`, `DATE_FORMAT_ISO = "ISO — 2026-11-08"` plus label `DATE_FORMAT = "Date format"` (all in `src/strings/planning.rs`). Auto → `date_order: null`, the others → `"mdy"` / `"dmy"` / `"ymd"`. Saved with `settings/save { date_order }` only (FEAT-1 merge-by-key; no other key sent). Every date shown afterwards (Services rows, Copy week prompt, print) uses the new `App::date_order()`.

- [ ] **Step 1: Write the failing UI tests** (`tests/ui_operator_flows.rs`): `copy_last_week_creates_dated_service_and_opens_it`, `copy_last_week_existing_date_offers_open`, `copy_last_week_blanks_checkbox_makes_blanks`, `services_sorted_by_date_undated_last`, `older_services_collapse_after_eight`, `services_dialog_scrolls_at_768`, `settings_date_order_override_changes_service_rows` (locale en-US; pick "UK — 8/11/2026" → Services row reads "Sun 8/11/2026"; pick Automatic → "Sun 11/8/2026"; only `date_order` changed in stored settings).
- [ ] **Step 2: Run** `FEAT_CARGO test --release --test ui_operator_flows -- copy_last_week services_ settings_date_order` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): Services dialog dates, Copy last week and date format setting`.

### Task B6: Template editor, template chooser, New from template (UI)

**Files:**
- Create: `ui/dialogs/template-editor.slint`, `ui/dialogs/template-pick.slint`, `src/ui/dialogs/template_editor.rs`
- Modify: `ui/dialogs/services.slint`, `src/ui/dialogs/services.rs`, `src/ui/dialogs/mod.rs` (`DialogKind::TemplateEditor(TemplateDraft)`)

**Interfaces:**
- `TemplateDraft { id: Uuid, name: String, day: Option<Day>, rows: Vec<(ServiceItem, bool /*keep*/, String /*label*/)> }` built from a service with `template_name_from_service`, `Day::of(date)` (Sunday if undated), `default_keep`; saving builds items with `template::template_items` and runs `template/save`.
- `TemplateEditor` (wide `DialogShell`): name, day `ComboBox` (Sundays…Saturdays, Any day), rows `[TemplateRow { icon, title, keep, label, want }]`, callbacks `toggle`, `label-edited`, `move`, `remove`, `add-blank` (`ADD_BLANK_CHOICES`), `save`. Reuses the service column drag code. Theme row only when FEAT-1 is in.
- Templates section in Services (name, `template_summary`, Edit / Rename / trash with `delete_template_q`); empty text `NO_TEMPLATES`. New from template: disabled + `NEED_TEMPLATE_TIP` with none; skip chooser with one; then name/date prompt (`new_service_name`, `default_date`) → `service/from-template`.

- [ ] **Step 1: Write the failing UI tests:** `save_as_template_defaults_songs_to_blank`, `template_editor_add_blank_and_reorder`, `new_from_template_disabled_without_templates`, `template_save_empty_name_shows_danger`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --test ui_operator_flows template` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): template editor and New from template`.

### Task B7: Template stage-layout references (FEAT-3 `LayoutRef::Template`)

Needs FEAT-3 (`src/stage/refs.rs`, `stage/layout/refs`, `stage/layouts/replace`). Spec §10 (FEAT-3 row): FEAT-6 adds the variant and its `layout_refs` arm; template refs are kept and reported.

**Files:**
- Modify: `src/stage/refs.rs` (FEAT-3's), `src/strings/stage.rs` (or wherever FEAT-3 keeps `delete_question` strings), `src/app.rs` (FEAT-3's `RefSources` builder; `stage/layouts/replace` must not clear template refs)

**Interfaces:**
- `LayoutRef::Template { template: Uuid, name: String }` (serde tag `"template"`, same `#[serde(tag = "kind", rename_all = "snake_case")]` as the other variants).
- `pub struct TemplateUse { pub template: Uuid, pub name: String, pub layouts: Vec<Uuid> }` and `RefSources.templates: &'a [TemplateUse]` (built in `app.rs` from `data.templates[*].items[*].stage_layout`).
- `layout_refs`: default first, then service items, then **one `Template` per template whose items name the layout at least once** (template order), then macros.
- `delete_question`: part `1 template` / `N templates` after `the stage default` and before macros; tail `the template keeps it until you edit the template` / `the templates keep it until you edit them`; services made from it later use the stage default. If FEAT-3's spec gives exact wording for this case, use FEAT-3's text instead and note it in the commit.
- `stage/layouts/replace` / `clear_deleted` do **not** touch template items: refs are kept (a service made from that template later gets `stage_layout: None` through `items_from_template`'s gone-layout rule, already tested in A5).

- [ ] **Step 1: Write the failing tests:** `layout_refs_include_templates` (two templates, one naming the layout twice → exactly one `Template` ref for it, after items and before macros), `delete_question_reports_templates` (exact text for 1 item + 1 template), `delete_layout_keeps_template_refs` (`stage/layouts/replace` deleting the layout leaves `data.templates[0].items[0].stage_layout == Some(id)`; then `service/from-template` gives that item `stage_layout: None`).
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- stage::refs app::` — Expected: FAIL (`LayoutRef::Template` not found).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): report template uses of stage layouts`.

### Task B8: Blank rows and fill mode (UI)

**Files:**
- Modify: `ui/operator/service.slint`, `ui/operator/slides.slint`, `src/ui/operator/*.rs` (service column, slides panel, keys), `ui/dialogs/add.slint`, `src/ui/dialogs/add.rs`, `src/ui/dialogs/add_media.rs`

**Interfaces:**
- Blank row: dashed `Theme.dash` border, `muted` text `kinds::row_text`. Slides panel for a selected blank: one large button `choose_button(want)`; double-click = button. Take Live / Enter on a blank → toast `BLANK_NOT_LIVE` (the core also refuses).
- Add dialog gains `fill: Option<Uuid>`: title `fill_title(want)`, opens on `add_tab_for(want)`, every add button reads `USE_THIS`; choosing builds the `ServiceItem` exactly as the add would and runs `service/fill`; on success closes and selects the item; on `BLANK_GONE`/`NOT_A_BLANK` toasts and stays open.

- [ ] **Step 1: Write the failing UI tests:** `blank_row_shows_choose_button_and_opens_add_in_fill_mode`, `fill_mode_use_this_replaces_blank`, `enter_on_blank_toasts_choose_first`, `fill_gone_blank_keeps_dialog_open`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --test ui_operator_flows -- blank fill` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): blank rows and Add dialog fill mode`.

### Task B9: Print core — adapter, `print/write` and report page (FEAT-6b)

**Files:**
- Create: `src/planning/print/adapter.rs` (`Data` + service → `PrintHeader`, `Vec<PrintItem>`)
- Modify: `src/app.rs` (`print/write`), `src/planning/report_page.rs` (add `write_and_open`) or FEAT-7's `src/report_page.rs` (see reconciliation), `Cargo.toml` (`opener`, exact version ≥ two weeks old, MIT OR Apache-2.0)

**Interfaces:**
- `pub fn print_input(data: &Data, service: &Service, order: DateOrder, today: NaiveDate, verses: &HashMap<Uuid, VerseText>) -> (PrintHeader, Vec<PrintItem>)`: song `order` from FEAT-4 `slides::item_order(item, song)` (or the song's arrangement before FEAT-4), `order_short` from FEAT-4 `SectionKind::short` (labels unchanged before FEAT-4); scripture code/second/copyright from FEAT-5 `bible_sources[].copyright`; media `MediaWhat` from `MediaKind`; blanks from `ItemKind::Blank`. Never reads `slide_notes`.
- `print/write {service, what, notes, scripture, song_per_page, paper, format: "html"|"pdf", path?}` → `{path}`: first gather verse text — stored text, then FEAT-5's per-translation cache, then `scripture::fetch(root, translation, input).await` for misses (failure → `VerseText::Failed`) — then `spawn_blocking` → `print::document` → `print::html`. No `path` → `report_page::write_and_open(store, report_stem(name), html)` (= `write_report(<data>/reports, …, Local::now().naive_local())` + `opener::open`; open failure → error `not_opened(path)` with the path for `CopyText`). With `path` → atomic write; error `save_failed`. `format: "pdf"` → "not available yet" until B20.
- **FEAT-7 reconciliation:** if `src/report_page.rs` already exists from FEAT-7, delete `src/planning/report_page.rs`, port its four tests into `src/report_page.rs` where missing, and call FEAT-7's `write_and_open(store, stem, html)`. Otherwise move ours to `src/report_page.rs` (spec path) with `write_and_open` added, so FEAT-7 finds it; keep `pub use crate::report_page;` in `src/planning/mod.rs` so B2's call still resolves.

- [ ] **Step 1: Write the failing tests:** `print_adapter_reads_every_kind`, `order_never_prints_slide_notes` (an item with `slide_notes` set → no block contains them), `lyrics_use_stored_or_cached_text_before_fetch` (a fake fetch that panics if called when text is stored/cached), `lyrics_fetch_failure_prints_reference_only` (fetch error → `Failed`), `print_write_without_path_writes_reports`, `print_write_with_path_writes_atomically`, `report_page_writes_to_reports_and_cleans_old`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- print report_page` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(print): print/write action, print adapter and report page`.

### Task B10: Print dialog (UI)

**Files:**
- Create: `ui/dialogs/print.slint`, `src/ui/dialogs/print.rs`
- Modify: `ui/operator/slides.slint` (Print… in the header menu), `ui/dialogs/services.slint` (Print… row action), `src/ui/dialogs/mod.rs` (`DialogKind::Print(Uuid)`); `rfd` reused from Plan 3

**Interfaces:**
- `PrintDialog { in property <bool> busy; in property <bool> pdf; … }` — what `Segmented`, three `CheckBox`es, paper `Segmented` (default from `App::paper()`), buttons `BTN_PRINT` (primary), `BTN_SAVE_HTML` (native `rfd` save dialog, default `save_file_name(..)`), `BTN_CLOSE`. Options saved with `settings/save { print }` only (FEAT-1 merge-by-key; B4's `settings_save_without_print_keeps_print` covers the other direction). Toasts `OPENED_TOAST`, `saved_toast`, `page_failed`, `not_opened` (with Copy), `save_failed`.

- [ ] **Step 1: Write the failing UI tests:** `print_dialog_remembers_options`, `print_dialog_saves_only_print_key` (other settings unchanged after closing the dialog), `print_save_html_writes_file` (`Ops` receives the path directly), `print_open_failure_shows_path`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --test ui_operator_flows print_` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS. Manual: print Both on Letter and A4 from Edge; page breaks sane.
- [ ] **Step 5: Commit** `feat(ui): Print or save dialog`.

### Task B11: Practice engine — versions, `Practice` state, start/stop/room (FEAT-6c, part 1)

**Files:**
- Create: `src/practice.rs` (`Practice` struct, `App` practice methods; re-exports `crate::planning::practice::*`)
- Modify: `src/live.rs` (`Live::starting_at`), `src/app.rs` (fields; actions `practice/start|stop|room`), `src/lib.rs` (`pub mod practice;`)

**Interfaces (spec §5.5):**
- `impl Live { pub fn starting_at(version: u64) -> Live }` (`new()` = `starting_at(0)`).
- `pub struct Practice { pub live: Live, pub started: Instant, pub last_command: Instant }` (spec §5.5) — `last_command` stays a monotonic `Instant`; `App::practice_idle() -> Option<Duration>` returns `last_command.elapsed()` (None when off). The UI's `Guard` (B18) takes that `Duration` as its `idle` argument (Task A13) — never a wall-clock conversion.
- `App.practice: Mutex<Option<Practice>>` (lock order data → live → practice → automation), `App.practice_floor: AtomicU64`; `practice_active()`, `with_engine(id, f) -> Result<T, String>` (`PRACTICE_ENDED` when off).
- `practice/start {logo_first}` (`Live::starting_at(start_version(real.version, floor))`; logo via `logo_id`; error `PRACTICE_ON`), `practice/stop {take?}` → `{shown}` (`take_target`; `Show` with `base = real.version`, id `take_id`; floor `floor_after`), `practice/room {what}` (id `room_id`; error `PRACTICE_OFF`). Broadcasting comes in B13; here the actions only change state.

- [ ] **Step 1: Write the failing tests** (`src/practice.rs`): `practice_versions_start_above_gap`, `second_practice_session_rejects_first_session_base`, `practice_base_is_stale_after_stop`, `start_with_logo_first_puts_logo_on_room`, `logo_first_hides_timer_keeps_message` (FEAT-1/2 overlays; pending until they are in), `practice_room_logo_changes_real_engine_only`, `stop_take_shows_practice_slide_on_room`, `stop_take_missing_item_only_stops`, `start_twice_errors_practice_on`, `room_button_when_off_errors_practice_off`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib practice` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(practice): practice engine state with start, stop and room actions`.

### Task B12: Practice routing — `classify`, `App::command` and macro steps (FEAT-6c, part 2)

**Files:**
- Modify: `src/practice.rs` (`classify`), `src/app.rs` (`App::command` dispatch; FEAT-2 `RunMacro` per-step routing)

**Interfaces:**
- `pub fn classify(cmd: &Command) -> CmdClass` — exhaustive `match` over every `Command` variant present at merge, no `_` arm (FEAT-1/2/3/4/7 variants mapped per the routing table).
- `App::command` dispatches on `route(classify(..))` while practice is on: `Both` → real first with the sent id, then practice with `practice_id(id)`, sender gets the real `Outcome`; clears never blocked by a stale base. `Stage` → FEAT-3 stage state unchanged. `NotInPractice` (media/audio pause) → error `NOT_PLAYED` ("Not played in practice."), nothing changes. After every accepted practice command: `practice.live.report(RoomEvent::Committed { version })`, `last_command = Instant::now()`.
- **The practice engine never produces a `room::Plan`** (spec §5.5): only real-engine commits reach the room planner.
- `RunMacro` uses `step_route` per step: `Both` / `Practice` / `Stage` as above; `Refuse` → `step_not_in_practice(name, n)` toast and the step is skipped.

- [ ] **Step 1: Write the failing tests:** `practice_next_does_not_change_real_engine`, `practice_commands_commit_immediately`, `practice_f1_clears_room_and_practice`, `practice_f2_to_f6_reach_room_and_practice`, `practice_off_and_hide_commands_reach_both_engines`, `practice_overlay_on_and_show_message_stay_in_practice`, `practice_jump_and_refresh_use_practice_engine`, `practice_set_screen_view_goes_to_real_engine`, `macro_clear_step_in_practice_reaches_room`, `macro_stage_step_in_practice_goes_to_stage_state`, `macro_in_practice_runs_on_practice_only`, `practice_resend_same_id_is_duplicate`, `real_base_is_stale_in_practice`, `room_event_during_practice_goes_to_real_engine`, `media_pause_in_practice_errors`, `message_in_practice_stays_in_practice_frame`, `practice_commits_never_build_room_plan` (after practice Next/Show/overlay commands the room planner's input — the real engine's committed version and its `room::Plan` source — is unchanged; only a `Both` clear produces a new plan), `classify_covers_every_command`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- practice classify macro_` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS. Tests that need FEAT-1/FEAT-2 commands are written when those FEATs are in; list the missing ones in the commit message.
- [ ] **Step 5: Commit** `feat(practice): clear-always-reaches-room command routing`.

### Task B13: Practice views — `Update::Practice`, `LiveView` fields, broadcast (FEAT-6c, part 3)

**Files:**
- Modify: `src/live.rs` (`LiveView.practice`/`room`), `src/app.rs` (`Update::Practice`, `broadcast_live`, `practice_view`, `display_view`), `src/practice.rs` (`room_look(frame, connected) -> RoomLook`)

**Interfaces:**
- `Update::Practice(Option<LiveUpdate>)`; `LiveView.practice: bool` (`skip_serializing_if = "Not::not"`) and `room: Option<String>` (skip if none).
- `practice_view() -> Option<LiveView>` (`practice: true`, `room: Some(strip_text(&room_look(real committed frame)))`); `display_view()` = practice view when on, else the real one.
- `room_look`: `title`/`slide` from the real committed frame's slide; `media` from its media layer; `background` when an item background is up; **`overlays = frame.overlays.len()`** (FEAT-1 `OverlaySlot`s; 0 before FEAT-1); `logo` from `frame.logo`; `music` = audio layer present and not paused; `connected` = a room window or room stream exists.
- `broadcast_live()` sends `Live(real)` then, when on, `Practice(Some(view))`; `practice/stop` broadcasts `Practice(None)` then `Live(real)`. Any data edit during practice rebroadcasts the practice view.

- [ ] **Step 1: Write the failing tests:** `stop_broadcasts_practice_none_then_real_live`, `edit_during_practice_rebroadcasts_practice_view`, `live_view_json_unchanged_when_not_practising`, `display_view_is_practice_while_on`, `practice_view_room_text_counts_overlays` (FEAT-1; pending until it is in).
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- practice live_view display_view` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(practice): practice view updates and display view`.

### Task B14: Practice and automation — per-engine `AutoState`, video end rule (FEAT-6c, part 4)

Needs FEAT-2's `AutomationRuntime`, `AutoState`, `AutoRun`, `EndAction`.

**Files:**
- Modify: `src/app.rs` / FEAT-2's `src/automation/runtime.rs` (the `auto` map), `src/practice.rs`

**Interfaces:**
- **Name settled with FEAT-2:** FEAT-2's plan bundles the run and the paused item as `AutoState { run: Option<AutoRun>, paused_item }` (its decision 5) and adds `AutomationRuntime.auto: HashMap<EngineId, AutoState>` once `EngineId` exists. FEAT-6 uses exactly that (`HashMap<EngineId, AutoState>`; the spec's `HashMap<EngineId, AutoRun>` is the same map under FEAT-2's name). `Internal(id)` commands reconcile through `App::with_engine(id)`; `practice/stop` removes `auto[Practice]`.
- Real video `EndAction::Next` (FEAT-2) acts as `Stop` while practice is on (the real engine must not follow the operator's practice cursor).
- `RoomRunning` for the start prompt (B17) is built here: `timer_overlay` / `message_overlay` = the real frame has an `OverlaySlot` with `OverlaySource::Timer { .. }` / `OverlaySource::Message { .. }`; `countdown` = FEAT-2 `TimersView.timers` has a running timer; `loop_` = `AutomationRuntime.pre.is_some()`; `music` as in B13; `video_next` = real media is a video with `EndAction::Next`. Function: `pub fn room_running(app: &App) -> RoomRunning`.

- [ ] **Step 1: Write the failing tests:** `auto_runs_keyed_per_engine`, `practice_auto_advance_dropped_on_stop`, `real_loop_keeps_running_on_room_during_practice`, `real_video_end_next_during_practice_does_not_follow_service_switch`, `room_running_reads_timer_and_message_overlays`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- auto_ real_ room_running` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(practice): per-engine automation and real video end rule`.

### Task B15: Followers — server streams and phone

**Files:**
- Modify: `src/server.rs` (per-role map; `remote_view` reads the active engine, adds `"practice": true`, `"room"`; action allowlist), phone page (`web/remote.html`/`remote.js` or Plan 4's replacement: banner `PHONE_BANNER` first on Control tab, room text), `web/stage.html` if it still exists (PRACTICE tag from `view.practice`)

**Interfaces:**
- Room stream matches only `Update::Live`; stage stream follows `display_view()` with the `practice` flag; remote follows the active engine.
- **None of the new actions are on the LAN server** (spec §5): `service/copy-week`, `service/from-template`, `service/set-date`, `service/fill`, `template/*`, `print/write`, `practice/start|stop|room` are rejected by the server's action allowlist like any other non-phone action.

- [ ] **Step 1: Write the failing tests:** `web_room_stream_ignores_practice`, `web_stage_stream_follows_practice_with_flag`, `remote_view_uses_practice_engine_and_version`, `remote_json_has_practice_and_room_only_while_on`, `phone_next_during_practice_advances_practice_not_room`, `phone_clear_during_practice_reaches_room`, `remote_timers_update_sends_timers_only`, `remote_allowlist_matches_feat2_table`, `planning_actions_not_on_lan_server` (each action above posted to the server → rejected, data unchanged).
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- server:: remote_ phone_` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(practice): server streams and phone follow practice; room stays real`.

### Task B16: Followers — native room and stage windows

**Files:**
- Modify: `src/ui/room.rs` (match only `Update::Live`; Lagged → `live_view()`), `src/ui/stage.rs` (`display_view()` on first show and Lagged; handle `Update::Practice` and `Update::Live`; set FEAT-3's `practice` property; for-now layout clears on a live-item change of the displayed engine)

- [ ] **Step 1: Write the failing tests:** `native_room_ignores_practice_updates`, `stage_lagged_resync_uses_display_view`, `stage_follows_practice_and_live_updates`, `stage_for_now_layout_clears_on_displayed_engine_item_change`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib -- ui::stage ui::room` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(practice): native room ignores practice; stage follows it`.

### Task B17: Practice UI — start prompt and practice strip

**Files:**
- Create: `ui/dialogs/practice.slint` (`StartPractice`), `ui/operator/practice-strip.slint` (`PracticeStrip`), `src/ui/dialogs/practice.rs`, `src/ui/operator/practice.rs` (strip, 160×90 room monitor drawn at 64×36)
- Modify: `ui/theme.slint` (`practice: #4aa3ff`, `practice-ink: #06121f`, `practice-strip-bg: #4aa3ff26`), `ui/icons.slint` (`practice`), `ui/operator.slint`, `ui/operator/topbar.slint` (Practice `TopBtn`, icon-only below 1500 px, tooltip `PRACTICE_TIP`)

**Interfaces:**
- `StartPractice { in property <string> room-text; in property <[string]> running; in property <bool> has-logo; in property <bool> hides-countdown; in property <bool> message-stays; in property <bool> music; }` fed from `showing_sentence(&room_look)`, `running_lines(&room_running(app))`, `primary_button`, `start_notes` (B13 `room_look`, B14 `room_running`); shown only when `showing_something`.
- `PracticeStrip { in property <image> room; in property <string> room-text; in property <bool> warn; callback room-logo(); callback room-clear(); callback end(); callback end-and-show(); callback keep(); }`, one 44 px row (FEAT-1 §7.2 budget; Up next gives up the height), Room ▾ menu below 1500 px. Room monitor re-renders only on `Update::Live`. Real-room notices → error toasts.

- [ ] **Step 1: Write the failing UI tests:** `practice_button_starts_without_prompt_when_room_empty`, `practice_prompt_when_room_showing_slide`, `practice_prompt_lists_running_loop`, `practice_prompt_says_clear_when_no_logo`, `practice_prompt_notes_timer_and_message`, `practice_strip_shows_room_text_and_end_button`, `practice_strip_room_logo_button`, `practice_strip_fits_1366x768`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --test ui_operator_flows -- practice_button practice_prompt practice_strip` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): practice start prompt and strip`.

### Task B18: Practice UI — Live column, preview and the forgotten-practice guard

**Files:**
- Modify: `ui/operator/live.slint` (PRACTICE header and outlines; "Not played in practice" caption; transport buttons disabled), `src/ui/preview.rs` (no mpv player for practice media; thumbnail + `NOT_PLAYED`), operator follower (`OpState.practice: Option<LiveUpdate>`; `apply_live` draws practice when `Some`; bases from the drawn view; Lagged reads both views), `src/ui/operator/practice.rs` (`Guard` timer)

**Interfaces:**
- `OpState.guard: Guard` (UI state, reset on each practice start); checked every 30 s: `guard.should_warn(Local::now().naive_local(), starts_at, app.practice_idle().unwrap_or_default())` where `starts_at` = current service `date` + FEAT-2 `starts_at` as local `NaiveDateTime` (`None` when either is missing); Keep practising → `Guard::keep(now, starts_at)`. `idle` comes from the `Instant` via `App::practice_idle()` (A13, B11) — no clock conversion.
- Media slides in practice: Live monitor shows the thumbnail with `NOT_PLAYED`; **transport buttons (play/pause/seek) are disabled** while the displayed engine is practice (spec §3.7).

- [ ] **Step 1: Write the failing UI tests:** `practice_live_header_says_practice`, `practice_f1_clears_room_and_practice`, `practice_warning_after_idle_hour`, `end_practice_and_show_this_takes_slide_live`, `practice_video_slide_shows_not_played_caption`, `practice_media_transport_disabled`, `end_practice_restores_real_live_view`, `stage_shows_practice_tag`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --test ui_operator_flows -- practice_live practice_f1 practice_warning practice_video practice_media end_practice stage_shows_practice` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): practice live column, media preview and guard`.

### Task B19: Manual checks

**Files:** none (results recorded in `docs/superpowers/checks/2026-native-checks.md` under a new "FEAT-6" heading)

- [ ] **Step 1:** Run spec §9.3 checks 1–7 on the church laptop and an old laptop (Windows, macOS, Linux once each): Copy last week with a future service present, then with the OS region set to UK, then with Settings → Date format overriding it; template → next Sunday → fill every blank; print Both on Letter and A4 from Edge/Safari/Firefox (Spanish song; Korean where a font exists); practice with projector and walk-in loop (room keeps looping, Next leaves room alone, F1 clears both, Room: Logo room-only, stage PRACTICE tag, phone banner/Next/Clear); End practice and show this; kill during practice and restart; format-2 build on FEAT-6 data opens read-only and the 2 → 3 backup exists.
- [ ] **Step 2:** Record PASS/FAIL per check with OS and date.
- [ ] **Step 3: Commit** `docs(checks): FEAT-6 manual checks`.

### Task B20 (follow-up, FEAT-6b-2): Save PDF via `krilla`

**Files:**
- Create: `src/planning/print/pdf.rs`, `tests/fixtures/fonts/` (small OFL Hangul subset font + its licence)
- Modify: `Cargo.toml` (`krilla`, exact version ≥ two weeks old, MIT OR Apache-2.0), `src/app.rs` (`print/write` `format: "pdf"` requires `path`), `ui/dialogs/print.slint` (`pdf` property → `BTN_SAVE_PDF`)

**Interfaces:**
- `pub fn pdf(doc: &PrintDoc, fonts: &render::Fonts) -> Result<Vec<u8>, String>`: cosmic-text `Buffer`s at column width (body 11 pt, headings 15 pt semibold, credits 8.5 pt; margins 0.75 in / 19 mm); a `Section` never splits if it fits on one page; each glyph run drawn with the face cosmic-text chose; Letter 8.5 × 11 in, A4 210 × 297 mm; footer "Printed from RustyPresenter · <date>" and "Page n of m"; title metadata = service name only. Failure → `pdf_failed(reason)` toast (6 s).

- [ ] **Step 1: Write the failing tests:** `pdf_has_page_count`, `pdf_letter_and_a4_media_boxes`, `pdf_embeds_fixture_font_for_hangul`.
- [ ] **Step 2: Run** `FEAT_CARGO test --release --lib planning::print::pdf` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `FEAT_CARGO test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(print): Save PDF with embedded fonts`.
