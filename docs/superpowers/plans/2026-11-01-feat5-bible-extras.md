# FEAT-5 — Bible Extras (Keyword Search, Two Translations, Slide Breaks) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Volunteers can find a verse by its words, show a passage in two translations at once (side by side or stacked, with both copyright notices), and choose where scripture slides break, with automatic breaks decided by the real slide layout on a planner thread instead of the 330-character estimate.

**Architecture:** All FEAT-5 logic lives in one new core module, `src/scripture/` (plus `src/strings/scripture.rs`). **Part A** builds it now as pure, unit-tested code that touches no shared file except one `pub mod scripture;` line in `src/lib.rs`: data types with serde defaults, verse-label parsing and Bible-box classification, the `bible_sources` adapter, the bounded text cache, the keyword index and search service, break marks, the measured slide planner (behind a `Measure` trait, with a deterministic `CharMeasure`), the position mapper, the plan cache and planner thread, the two-translation rules, item validation and load fix-ups, and the parallel-slide geometry. Types the spec puts on existing structs (`ItemKind::Scripture`, `Slide`, `SlideContent::Text`, `Settings`, `DisplayRoles`) are standalone in `scripture::types` now and are re-exported from `model.rs` in Part B, so they never move. **Part B** wires them into model/store/app/slides/live/render and the Plan 3 Slint UI; it is blocked until the `native-foundation` branch has Plans 3 and 4 merged.

**Tech Stack:** Rust 2024, existing crates only (`serde`, `serde_json`, `uuid`, `tokio` with `rt-multi-thread`/`macros`, `cosmic-text` in Part B). No `Cargo.toml` change. Read-only use of existing public items: `bible::{kjv, find_book, parse_reference, Reference, BOOKS}` and `model::VerseText` (a two-field `{reference, text}` struct that Plans 3/4 do not change). Part A does **not** import `slides::join_refs` or `model::Theme`, because Plan 3/4 and FEAT-1 rewrite those files: `join_refs` is copied into `scripture::planning::refs` (Task A9), and planning is generic over a small `PlanTheme` trait (Task A9) that Part B implements for the real theme type. Part B adds Slint 1.18 UI on Plan 3's file layout.

**Spec:** `docs/superpowers/specs/2026-11-01-feat5-bible-extras-design.md` (revision 3). Also read: v1 spec §5.4, §6, §7, §8 (`docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md`); FEAT-1 §4.7 (`Data::after_load`), §5.3 (merge by key), §7 (key and Settings tab tables), §7.2 (layout budget); FEAT-4 §7 (strings convention, `Slide::new`, reconcile, Command::Refresh); Plan 3 (`docs/superpowers/plans/2026-11-01-native-operator.md`) Tasks 7, 11, 16, 18, 20, 22.

## Global Constraints

- **Part A file rule.** Part A creates only new files: `src/scripture/**` and `src/strings/scripture.rs`. The single edit to an existing file is one `pub mod` line: `pub mod scripture;` in `src/lib.rs` (plus, only when `src/strings/mod.rs` already exists, `pub mod scripture;` there instead of the `#[path]` include — see the next rule). No edits to `src/app.rs`, `src/live.rs`, `src/model.rs`, `src/store.rs`, `src/room.rs`, `src/slides.rs`, `src/server.rs`, `src/render/**`, `src/ui/**`, `ui/**`, `web/**`, `src/bible*`, `Cargo.toml`.
- **Strings path.** `src/strings/mod.rs` and its `pub mod strings;` line in `src/lib.rs` are **shared scaffold, created by whichever FEAT lands first** (FEAT-1 and FEAT-7 Global Constraints). When Task A1 runs:
  - if `src/strings/mod.rs` **already exists**, add `pub mod scripture;` to it and put `pub use crate::strings::scripture as strings;` in `src/scripture/mod.rs` (no `#[path]` anywhere);
  - otherwise, `src/scripture/mod.rs` includes the file with `#[path = "../strings/scripture.rs"] pub mod strings;` and Part B Task B1 switches to the shared path. If another FEAT's Part A creates `src/strings/mod.rs` before Part B, whoever merges second makes the B1 switch at merge time, so the file is never compiled under two module paths.
  Either way call sites use `scripture::strings::…` and never change.
- **Forward-compatible enums (cross-spec Lenient convention, FEAT-2 Global Constraints).** Every persisted unit enum puts `#[default] #[serde(other)]` on its default variant, so an older build reads a newer build's variant as the default instead of failing to parse `data.json`. In this plan: `ParallelLayout` (`SideBySide`), `Order` (`Best`), and `SourceStatus` (`Unknown`, not the default but the catch-all).
- **Translation status.** Every function that asks "is this translation offline / usable?" takes a `&Sources` (Task A3): the `bible_sources` rows **plus** `local`, the codes stored on this computer (KJV and every bundled/imported translation from `ImportedBible`). A translation with no row is `Offline` when it is in `local`, else `Unavailable` (spec §4: "when `bible_sources` is not known yet, KJV and bundled/imported translations count as offline").
- **Every user-facing string** is a `pub const` or a small `pub fn` in `src/strings/scripture.rs`, copied verbatim from the spec (§3, §5, §8). No literal user text anywhere else in `src/scripture/**`.
- **Nothing under `src/bible*` is added or changed.** The Bible session's readers are reached only through the `BibleReaders` trait (Task A4); the Part A implementation `KjvOnly` uses `bible::kjv()` only.
- **Copyright rule (spec §5.1a).** Verse text of a translation whose status is not `offline` is never serialized into item data; it lives only in `TextCache` (memory + `<root>/scripture-cache/<TR>.json`, capped at `cache_cap` = 500 verses). Online translations are never indexed, pre-warmed or bulk-fetched.
- **No measuring in the command path.** Only the planner thread (Task A12b) and the breaks dialog worker call a real `Measure`; `plan_or_estimate` uses `CharMeasure` on a miss.
- **Numbers pinned by the spec:** search debounce 250 ms; snippet 160 chars, start ≤ 40 chars before the match; result cap 200; ≤ 12 query terms; prefix matching for terms of ≥ 3 letters; proximity window `2 × terms + 2`; OT = books 0–38, NT = 39–65; `MIN_PX` 26; `READABLE = max(0.75 × resolved scripture_size, 40)` with `DEFAULT_SCRIPTURE_SIZE` 64; per-item limit `min(400, cache_cap)`; planner fit LRU 4,096 entries; default canvas 1920×1080; gutter 4 % canvas width; divider 2 px at 25 % alpha; heading `REFERENCE_PX × 0.8` (REFERENCE_PX 44) weight 600 at 70 % alpha; footer `REFERENCE_PX × 0.7`, shrinking to `MIN_PX × 0.6`.
- **Serde.** Every new field has a default; old `data.json` loads unchanged; no data-format bump (`SUPPORTED_FORMAT` is FEAT-1's). `ParallelLayout` is `"side_by_side"` / `"stacked"`; `VerseId` is `[book, chapter, verse]`.
- **Tests** live in each file's `#[cfg(test)] mod tests`. Part A test command (PowerShell), with `<path>` the module path given in each task:
  `& "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd" test --release --lib <path>`
  Below it is written `feat-cargo test --release --lib <path>`. Every Part A task ends with `feat-cargo test --release --lib scripture` passing as well. `cargo test` takes one name filter before `--`; to run several modules, put them after `--` (`feat-cargo test --release --lib -- a:: b::`).
- **Every listed test has a body.** The code blocks give each test's inputs and `assert!`/`assert_eq!` lines; an implementer writes exactly those and never leaves a `#[test] fn x() {}` (an empty test passes and proves nothing). Shared test helpers (`tmp_root`, `verses`, `rows`, `FakeReaders`, `fetcher_with_fake`, `v`, `input`, `ids`, `set`) live in `src/scripture/test_support.rs`, declared `#[cfg(test)] pub(crate) mod test_support;` in `src/scripture/mod.rs` (Task A3).
- **Cross-plan entry point.** FEAT-6 (`print/write`) and FEAT-7 (`service/import-pco`) call the spec §5.1 free function `crate::scripture::fetch(root: &Path, translation: &str, input: &str) -> Lookup` (async). Task A4 provides it with exactly that signature, plus `fetch_blocking` for callers already on the blocking pool; Task B2a installs the app's context behind it. Do not rename it.
- **File size:** each file stays under about 400 lines; split a file into a directory module before it grows past that.

## Review Focus

1. **Punctuation inside verse words** (`LORD's`, `know—that`, `Lord:`). Expected: the same tokenizer splits verses and queries on whitespace and the dashes `- – —`, then normalises, so `lord` finds `LORD's` and `know` finds `know—that`. Test `tokenizer_splits_dashes_and_drops_punctuation` in Task A5.
2. **A `bible_sources` status this build doesn't know** (the Bible session adds `"beta"`). Expected: it loads as `SourceStatus::Unknown`, counts as not usable and not searchable, and fetch says "NIV isn't available." — never a panic or a failed state parse. Test `unknown_status_loads_as_unknown_and_is_not_usable` in Task A3. Related: a WEB item loaded before `bible_sources` arrives keeps its stored text (`after_load_keeps_web_text_when_sources_unknown`, Task A14).
3. **A cache file that is corrupt or from another version.** Expected: the cache starts empty for that translation, the bad file is replaced on the next write, and fetch falls through to the online reader. Test `corrupt_cache_file_starts_empty` in Task A3.
4. **A search reply for an older query arriving after a newer one** (fast typing on an old laptop). Expected: only the newest sequence number is applied. Test `seq_guard_drops_older_replies` in Task A11.
5. **Planning a passage whose labels don't parse** (an item saved by an older build with odd labels). Expected: verses keep their stored order after the previous parsed verse, the slide gets `source: None`, the gap is disabled, and nothing panics. Test `unparseable_labels_plan_in_order_without_source` in Task A9 and `gap_rows_disable_unparseable_verse` in Task A8.
6. **Changing an item's translation when a manual break sits on a verse the new translation lacks.** Expected: the `before` mark moves to the nearest existing boundary (spec §5.4) instead of being dropped. Test `service_update_retranslate_moves_before_mark` in Task A14.

---

# PART A — Core, buildable now

### Task A1: Module skeleton, strings and shared types

**Files:**
- Modify: `src/lib.rs` (add `pub mod scripture;` — the only edit to an existing file in Part A); `src/strings/mod.rs` **only if it already exists** (add `pub mod scripture;`)
- Create: `src/scripture/mod.rs`, `src/scripture/types.rs`, `src/strings/scripture.rs`

**Interfaces:**
- Produces (`scripture::types`, all re-exported from `scripture`):
  - `VerseId(pub u8, pub u16, pub u16)` — derives `Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash`; serialises as `[b, c, v]` (tuple struct does this by default). `impl VerseId { pub fn book(self) -> u8; pub fn chapter(self) -> u16; pub fn verse(self) -> u16 }`.
  - `ParallelLayout { #[default] #[serde(other)] SideBySide, Stacked }`, `rename_all = "snake_case"`, derives `Copy, Eq, Hash, Default`. The `#[serde(other)]` makes a layout added by a newer build (`"three_column"`) load as `SideBySide` instead of failing the whole `data.json` parse.
  - `SecondText { translation: String, #[serde(default, skip_serializing_if = "Vec::is_empty")] verses: Vec<VerseText> }`.
  - `VerseBreaks { before: BTreeSet<VerseId>, join: BTreeSet<VerseId> }` with `#[serde(default)]`, `is_empty()`.
  - `SlideSource { first: VerseId, last: VerseId, part: u8 }`.
  - `ParallelText { main_code, second_code, text: String, layout: ParallelLayout }`.
  - `BibleSettings { second_translation: Option<String>, layout: ParallelLayout }`, `#[serde(default)]`.
  - `ScriptureFields` — the full new shape of `ItemKind::Scripture`, as a struct with exactly the spec §4 field attributes: `reference`, `translation`, `verses: Vec<VerseText>`, `#[serde(default)] per_slide: u8`, `#[serde(default, skip_serializing_if = "Option::is_none")] second: Option<SecondText>`, `#[serde(default)] layout: ParallelLayout`, `#[serde(default, skip_serializing_if = "VerseBreaks::is_empty")] breaks: VerseBreaks`. Part B Task B1 adds the same attributes to the enum variant and proves the JSON is identical.
  - `UPGRADE_NOTE_KEY: &str = "feat5-measured-breaks"`.
- Produces (`scripture::strings`, file `src/strings/scripture.rs`): every message in spec §8 and §5.1 / §5.2 wording. Constants: `EMPTY_SEARCH`, `ADD_A_CHAPTER` ("Add a chapter, like Job 1 or Job 1:1."), `PICK_DIFFERENT_SECOND`, `SECOND_TURNED_OFF`, `TOO_LONG_CHIP`, `JOIN_TOO_LONG_NOTE`, `UPGRADE_TOAST`, `FITTING_CHIP` ("Fitting slides…"), `UNKNOWN_TRANSLATION` ("Unknown translation."), `STATUS_OFFLINE` ("Ready on this computer"), `STATUS_ONLINE` ("Ready (needs internet)"), `STATUS_NEEDS_KEY` ("Needs a key — Get one"), `STATUS_UNAVAILABLE` ("Not available"), `SEARCH_CHIP_READY` ("Search: Ready"), `SEARCH_CHIP_ESV` ("Search: online (ESV)"), `UNPARSEABLE_VERSE_TOOLTIP`, `HELP_SEARCH_RULES: &[&str]` (the seven bullets of spec §3.1, verbatim), `ALSO_SHOW` ("Also show…"), `SEARCH_BY_WORDS` ("Search by words…"), `SLIDE_BREAKS` ("Slide breaks"), `CHOOSE_SLIDE_BREAKS` ("Choose slide breaks…"), `RESET_TO_AUTOMATIC`, `GAP_AUTO` ("New slide (automatic)"), `GAP_ADDED` ("New slide (you added)"), `GAP_KEPT` ("Kept together"), `GAP_NONE_HOVER` ("+ New slide here"), `LOWER_THIRD_STACKS` ("Lower-third themes always stack two translations."), `SEARCH_THESE_WORDS` ("Search for these words"). Functions (`tr` is a translation code): `no_hits(tr)`, `no_hits_one_word(tr)`, `too_many_hits(shown: usize, total: usize)` (formats the total with a thousands comma: "Showing the first 200 of 1,240 verses — add more words to narrow it down."), `found(n, tr)` ("3 verses found in KJV"; "1 verse found in KJV"), `index_building(tr)`, `search_unavailable(tr)`, `searching_instead(tr, local)`, `esv_offline()`, `search_chip_none(tr)`, `needs_key(tr)`, `not_available(tr)`, `text_unreachable(tr)`, `main_only_added(second, main)`, `couldnt_get(tr, msg)`, `missing_verse(tr, reference)`, `added_toast(label)` ("Psalm 46:10–11 added — press Enter to show it"), `add_button(label)` ("Add Psalm 46:10–11"), `search_for_instead(word)` ("Search for \"job\" instead"), `passage_too_long(limit)` ("That passage is too long. Choose up to about {limit} verses.").

- [ ] **Step 1: Write the failing tests** (`src/scripture/types.rs`, `src/strings/scripture.rs`)

```rust
// types.rs
const OLD: &str = r#"{"reference":"John 3:16","translation":"KJV","verses":[{"reference":"John 3:16","text":"For God so loved the world"}],"per_slide":0}"#;

#[test]
fn old_scripture_item_loads_with_defaults() {
    let f: ScriptureFields = serde_json::from_str(OLD).unwrap();
    assert_eq!((f.second, f.layout, f.breaks.is_empty()), (None, ParallelLayout::SideBySide, true));
}
#[test]
fn verse_breaks_is_empty_skips_serializing() {
    let f: ScriptureFields = serde_json::from_str(OLD).unwrap();
    let v = serde_json::to_value(&f).unwrap();
    assert!(v.get("breaks").is_none() && v.get("second").is_none());
}
#[test]
fn parallel_layout_serializes_snake_case() {
    assert_eq!(serde_json::to_string(&ParallelLayout::SideBySide).unwrap(), "\"side_by_side\"");
    assert_eq!(serde_json::to_string(&ParallelLayout::Stacked).unwrap(), "\"stacked\"");
}
#[test]
fn verse_id_serializes_as_triple_and_breaks_round_trip() {
    assert_eq!(serde_json::to_string(&VerseId(42, 3, 16)).unwrap(), "[42,3,16]");
    let b: VerseBreaks = serde_json::from_str(r#"{"before":[[42,3,17]]}"#).unwrap();
    assert_eq!(b.before.iter().copied().collect::<Vec<_>>(), vec![VerseId(42, 3, 17)]);
    assert!(b.join.is_empty());
}
#[test]
fn bible_settings_from_empty_object_is_default() {
    assert_eq!(serde_json::from_str::<BibleSettings>("{}").unwrap(), BibleSettings::default());
}
#[test]
fn second_text_offline_verses_round_trip_and_online_omits_empty() {
    let s = SecondText { translation: "WEB".into(), verses: vec![VerseText { reference: "John 3:16".into(), text: "For God so loved".into() }] };
    let back: SecondText = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    let online = SecondText { translation: "NIV".into(), verses: vec![] };
    assert_eq!(serde_json::to_string(&online).unwrap(), r#"{"translation":"NIV"}"#);
}
#[test]
fn unknown_parallel_layout_loads_as_side_by_side() {
    assert_eq!(serde_json::from_str::<ParallelLayout>("\"three_column\"").unwrap(), ParallelLayout::SideBySide);
    let s: BibleSettings = serde_json::from_str(r#"{"second_translation":"NIV","layout":"three_column"}"#).unwrap();
    assert_eq!((s.second_translation.as_deref(), s.layout), (Some("NIV"), ParallelLayout::SideBySide));
    let f: ScriptureFields = serde_json::from_str(&OLD.replace("\"per_slide\":0", "\"per_slide\":0,\"layout\":\"three_column\"")).unwrap();
    assert_eq!(f.layout, ParallelLayout::SideBySide);
}

// strings/scripture.rs
#[test]
fn too_many_hits_formats_thousands() {
    assert_eq!(too_many_hits(200, 1240), "Showing the first 200 of 1,240 verses — add more words to narrow it down.");
}
#[test]
fn found_is_singular_for_one() { assert_eq!(found(1, "KJV"), "1 verse found in KJV"); assert_eq!(found(3, "KJV"), "3 verses found in KJV"); }
#[test]
fn main_only_message_names_both() {
    assert_eq!(main_only_added("NIV", "KJV"), "NIV isn't available right now, so only KJV was added. Turn off the second translation with the X.");
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::types` — Expected: FAIL to compile (`scripture` module missing).
- [ ] **Step 3: Implement** the types and strings. First check whether `src/strings/mod.rs` exists (`Test-Path src/strings/mod.rs`):
  - **exists:** add `pub mod scripture;` to `src/strings/mod.rs`; `src/scripture/mod.rs` declares `pub mod types;`, `pub use crate::strings::scripture as strings;` and `pub use types::*;`.
  - **missing:** do not create it; `src/scripture/mod.rs` declares `pub mod types;`, `#[path = "../strings/scripture.rs"] pub mod strings;` and `pub use types::*;`, with a comment `// FEAT-5 B1: switch to crate::strings::scripture once src/strings/mod.rs exists`.
  Later tasks add their `pub mod` lines to `src/scripture/mod.rs` (a new file, so allowed). Derive `PartialEq` on every type in `types.rs` (tests compare them).
- [ ] **Step 4: Run** `feat-cargo test --release --lib scripture` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): FEAT-5 types and strings module`

### Task A2: Verse labels and Bible box classification

**Files:**
- Create: `src/scripture/input.rs`

**Interfaces:**
- Consumes: `bible::{find_book, parse_reference, Reference, BOOKS}` (read-only), `VerseId`.
- Produces:
  - `pub fn parse_verse_label(label: &str) -> Option<(&str /*book part*/, u16, u16)>` — `rsplit_once(' ')`, then `c:v` both > 0.
  - `pub fn verse_id_of(label: &str) -> Option<VerseId>` — `find_book` on the book part (accepts "Psalm" and "Psalms").
  - `pub fn display_name(book: u8) -> &'static str` — `BOOKS[book].1`, with "Psalms" → "Psalm" (matches `Kjv::lookup` labels).
  - `pub fn label_of(id: VerseId) -> String` — `"{display_name} {c}:{v}"`.
  - `pub fn chapter_reference(book: u8, chapter: u16) -> String` — `"Psalm 46"`.
  - `pub enum BibleInput { Reference(Reference), BookOnly { book: u8 }, UnknownWithDigits, Words }` (derive `Debug, Clone, PartialEq`).
  - `pub fn classify_bible_input(text: &str) -> BibleInput` — the four rules of spec §5.6 in order: `parse_reference` Ok → Reference; `find_book(text.trim())` Some → BookOnly; any ASCII digit → UnknownWithDigits; else Words.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn verse_id_of_round_trips_display_names() {
    for (i, _) in BOOKS.iter().enumerate() {
        let label = format!("{} 1:1", display_name(i as u8));
        assert_eq!(verse_id_of(&label), Some(VerseId(i as u8, 1, 1)), "{label}");
        assert_eq!(label_of(VerseId(i as u8, 1, 1)), label);
    }
    assert_eq!(display_name(18), "Psalm");
}
#[test]
fn verse_id_of_parses_labels_from_kjv_and_imported_readers() {
    let (_, vs) = crate::bible::kjv().lookup("John 3:16-17").unwrap();
    assert_eq!(vs.iter().map(|v| verse_id_of(&v.reference)).collect::<Vec<_>>(), vec![Some(VerseId(42, 3, 16)), Some(VerseId(42, 3, 17))]);
    assert_eq!(verse_id_of("Psalms 23:1"), Some(VerseId(18, 23, 1)));
    assert_eq!(verse_id_of("Song of Solomon 2:1"), Some(VerseId(21, 2, 1)));
    assert_eq!(verse_id_of("1 Kings 22:3"), Some(VerseId(10, 22, 3)));
}
#[test]
fn unparseable_label_gives_none() {
    for l in ["Psalm 23", "Hezekiah 3:1", "John 3:0", "", "John :16", "John 3:16a"] { assert_eq!(verse_id_of(l), None, "{l}"); }
}
#[test]
fn classify_reference() { assert!(matches!(classify_bible_input("jn 3 16"), BibleInput::Reference(_))); }
#[test]
fn classify_book_only_job_mark_judge_numbers() {
    for (t, b) in [("Job", 17), ("Mark", 40), ("judge", 6), ("numbers", 3)] { assert_eq!(classify_bible_input(t), BibleInput::BookOnly { book: b }, "{t}"); }
}
#[test]
fn classify_digits_unknown_book() {
    for t in ["Hezekiah 3:1", "40 days", "seventy times 7"] { assert_eq!(classify_bible_input(t), BibleInput::UnknownWithDigits, "{t}"); }
}
#[test]
fn classify_words() {
    for t in ["be still and know", "Phillipians", "love one another"] { assert_eq!(classify_bible_input(t), BibleInput::Words, "{t}"); }
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::input` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** in `src/scripture/input.rs`; add `pub mod input;` to `src/scripture/mod.rs`.
- [ ] **Step 4: Run** `feat-cargo test --release --lib scripture::input` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): verse ids from labels and Bible box classification`

### Task A3: Translation sources and the text cache

**Files:**
- Create: `src/scripture/sources.rs`, `src/scripture/cache.rs`, `src/scripture/test_support.rs` (`#[cfg(test)]` helpers shared by later tasks)

**Interfaces:**
- Produces (`sources.rs`):
  - `#[serde(rename_all = "snake_case")] pub enum SourceStatus { Offline, Online, NeedsKey, Unavailable, #[serde(other)] Unknown }`.
  - `pub struct SourceInfo { translation: String, status: SourceStatus, #[serde(default)] provider: String, #[serde(default)] signup_url: Option<String>, #[serde(default)] copyright: Option<String> }` — one row of `state_json["bible_sources"]`.
  - `#[derive(Clone, Debug, Default)] pub struct Sources { pub rows: Vec<SourceInfo>, pub local: Vec<String> }` — the rows plus `local`, the translation codes stored on this computer (KJV and every bundled/imported translation; Part B fills it from `ImportedBible`). `pub fn kjv_only() -> Sources` (`rows: []`, `local: ["KJV"]`) — the value before `bible-esv` merges and before `bible_sources` is known.
  - `pub fn status_of(sources: &Sources, tr: &str) -> SourceStatus` — the row's status (codes compared case-insensitively); when there is no row: `Offline` if `tr` is in `sources.local`, else `Unavailable` (the spec §4 "bible_sources not known yet: KJV and bundled/imported translations count as offline" rule).
  - `pub fn is_usable(s: SourceStatus) -> bool` (Offline | Online).
  - `pub fn is_known(sources: &Sources, tr: &str) -> bool` — has a row or is in `local`.
  - `pub fn copyright_of(sources, tr) -> Option<String>` — `None` for missing or blank.
  - `pub fn status_wording(s) -> &'static str` (the four `STATUS_*` strings; Unknown → `STATUS_UNAVAILABLE`).
  - `pub fn provider_of(sources, tr) -> &str` (`""` with no row).
- Produces (`test_support.rs`, `pub(crate)`, test-only): `tmp_root() -> PathBuf` (unique dir under `std::env::temp_dir()`), `verses(book, chapter, range) -> Vec<VerseText>` (labels via `input::label_of`, text `"t{b}-{c}-{v}"`), `verses_many(book, chapters, per_chapter)`, `ids_of(&[VerseText]) -> BTreeSet<VerseId>`, `rows(&[(&str, &str)]) -> Sources` (rows from `(code, status)` pairs, `local: ["KJV"]`), `row(code, status, copyright: Option<&str>) -> SourceInfo`, `ids(&[(u8,u16,u16)]) -> Vec<VerseId>`, `set(&[(u8,u16,u16)]) -> BTreeSet<VerseId>`. Task A4 adds `FakeReaders` and `fetcher_with_fake`; Task A9 adds `v`, `input`, `TestTheme`.
- Produces (`cache.rs`):
  - `pub const CACHE_CAP_DEFAULT: usize = 500;` `pub fn cache_cap(provider: &str) -> usize` (500 for every provider today, ESV included).
  - `pub fn per_item_limit(cap: usize) -> usize` = `min(400, cap)`.
  - `pub struct TextCache` (interior `Mutex`, `Send + Sync`): `pub fn new(root: PathBuf) -> Self` (no I/O); `pub fn load(&self, tr: &str)` reads `<root>/scripture-cache/<TR>.json` into memory (missing or corrupt → empty); `pub fn get(&self, tr: &str, reference: &str) -> Option<(String, Vec<VerseText>)>` — **memory only**, passage key = `reference.trim().to_lowercase()`, hit only when every verse of the stored passage is still present, marks verses used; `pub fn put(&self, tr: &str, reference: &str, cap: usize, label: &str, verses: &[VerseText])` — stores in memory and rewrites the file; `pub fn set_pinned(&self, tr: &str, ids: BTreeSet<VerseId>)` (verses of the open service); `pub fn file_len(&self, tr) -> usize` (reads `<root>/scripture-cache/<TR>.json` from disk and counts its verses; 0 when missing or corrupt).
  - Eviction when writing the file: keep at most `cap` verses; drop least-recently-used **unpinned** first; if pinned alone exceed `cap`, the file holds the `cap` most recently used pinned verses and the rest stay in memory for the session.
  - Verses without a `VerseId` are not cached (they cannot be keyed); `put` keeps the passage only if all its verses have ids.

- [ ] **Step 1: Write the failing tests**

```rust
// sources.rs
#[test]
fn bible_sources_row_parses() {
    let r: SourceInfo = serde_json::from_str(r#"{"translation":"NIV","status":"needs_key","provider":"api.bible","signup_url":"https://example.org/signup"}"#).unwrap();
    assert_eq!((r.status, r.copyright.clone()), (SourceStatus::NeedsKey, None));
}
#[test]
fn unknown_status_loads_as_unknown_and_is_not_usable() {
    let r: SourceInfo = serde_json::from_str(r#"{"translation":"NIV","status":"beta","provider":"x"}"#).unwrap();
    assert_eq!(r.status, SourceStatus::Unknown);
    assert!(!is_usable(r.status));
}
#[test]
fn kjv_is_offline_before_sources_are_known() {
    assert_eq!(status_of(&Sources::kjv_only(), "KJV"), SourceStatus::Offline);
    assert_eq!(status_of(&Sources::kjv_only(), "NIV"), SourceStatus::Unavailable);
}
#[test]
fn local_translations_are_offline_before_sources_are_known() {
    let s = Sources { rows: vec![], local: vec!["KJV".into(), "WEB".into(), "BSB".into()] };
    assert_eq!((status_of(&s, "WEB"), status_of(&s, "bsb"), status_of(&s, "ESV")),
               (SourceStatus::Offline, SourceStatus::Offline, SourceStatus::Unavailable));
    let with_row = Sources { rows: vec![row("WEB", "unavailable", None)], ..s };
    assert_eq!(status_of(&with_row, "WEB"), SourceStatus::Unavailable);   // a row always wins
}
#[test]
fn copyright_comes_from_bible_sources() {
    let s = Sources { rows: vec![row("KJV", "offline", Some("")), row("NIV", "online", Some("Scripture quotations … NIV®")), row("ESV", "online", None)], local: vec![] };
    assert_eq!((copyright_of(&s, "KJV"), copyright_of(&s, "ESV"), copyright_of(&s, "CSB")), (None, None, None));
    assert_eq!(copyright_of(&s, "NIV").as_deref(), Some("Scripture quotations … NIV®"));
}

// cache.rs
#[test]
fn per_item_limit_is_min_of_400_and_cap() { assert_eq!(per_item_limit(500), 400); assert_eq!(per_item_limit(300), 300); }
#[test]
fn cache_file_capped_at_500_verses_for_esv() {
    let c = TextCache::new(tmp_root());
    c.put("ESV", "Psalm 119", cache_cap("esv"), "Psalm 119", &verses(18, 119, 1..=176));
    c.put("ESV", "Isaiah 1-8", cache_cap("esv"), "Isaiah 1–8", &verses_many(22, 1..=8, 50));   // 400 verses; 176 + 400 > 500
    assert_eq!(c.file_len("ESV"), 500);
}
#[test]
fn cache_evicts_outside_open_service_first() {
    let root = tmp_root();
    let c = TextCache::new(root.clone());
    let pinned = verses(18, 119, 1..=100);
    c.set_pinned("ESV", ids_of(&pinned));
    c.put("ESV", "Psalm 119:1-100", 500, "Psalm 119:1–100", &pinned);
    for ch in 1..=6 { c.put("ESV", &format!("Isaiah {ch}"), 500, &format!("Isaiah {ch}"), &verses(22, ch, 1..=100)); }
    let fresh = TextCache::new(root);
    fresh.load("ESV");
    assert_eq!(fresh.file_len("ESV"), 500);
    assert_eq!(fresh.get("ESV", "Psalm 119:1-100").unwrap().1.len(), 100);   // every pinned verse survived
    assert!(fresh.get("ESV", "Isaiah 1").is_none() && fresh.get("ESV", "Isaiah 2").is_none());   // oldest unpinned went first
    assert!(fresh.get("ESV", "Isaiah 6").is_some());
}
#[test]
fn pinned_overflow_kept_in_memory_only() {
    let c = TextCache::new(tmp_root());
    let vs = verses_many(19, 1..=6, 100);   // 600 verses
    c.set_pinned("ESV", ids_of(&vs));
    c.put("ESV", "Proverbs 1-6", 500, "Proverbs 1–6", &vs);
    assert_eq!(c.file_len("ESV"), 500);
    assert_eq!(c.get("ESV", "Proverbs 1-6").unwrap().1.len(), 600);
}
#[test]
fn get_is_memory_only_until_load() {
    let root = tmp_root();
    TextCache::new(root.clone()).put("ESV", "John 3:16", 500, "John 3:16", &verses(42, 3, 16..=16));
    let c2 = TextCache::new(root);
    assert!(c2.get("ESV", "John 3:16").is_none());
    c2.load("ESV");
    assert_eq!(c2.get("ESV", "  JOHN 3:16 ").unwrap().0, "John 3:16");   // key is trimmed + lowercased
}
#[test]
fn corrupt_cache_file_starts_empty() {
    for bad in ["not json", r#"{"v":99,"verses":[],"passages":{}}"#] {
        let root = tmp_root();
        std::fs::create_dir_all(root.join("scripture-cache")).unwrap();
        std::fs::write(root.join("scripture-cache/ESV.json"), bad).unwrap();
        let c = TextCache::new(root.clone());
        c.load("ESV");
        assert!(c.get("ESV", "John 3:16").is_none());
        c.put("ESV", "John 3:16", 500, "John 3:16", &verses(42, 3, 16..=16));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("scripture-cache/ESV.json")).unwrap()).unwrap();
        assert_eq!((v["v"].as_u64(), c.file_len("ESV")), (Some(1), 1));
    }
}
#[test]
fn verses_without_ids_are_not_cached() {
    let c = TextCache::new(tmp_root());
    c.put("ESV", "Odd", 500, "Odd", &[VerseText { reference: "Odd label".into(), text: "x".into() }]);
    assert!(c.get("ESV", "Odd").is_none());
    assert_eq!(c.file_len("ESV"), 0);
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::cache` and `… scripture::sources` — Expected: FAIL (modules missing).
- [ ] **Step 3: Implement.** File format (internal): `{"v":1,"verses":[{"id":[b,c,v],"reference":"…","text":"…","used":u64}],"passages":{"<key>":{"label":"…","ids":[[b,c,v],…]}}}`; any other `v` is treated as corrupt. Write through a temp file + rename. Recency is a monotonic `u64` tick in the `Mutex`. Create `src/scripture/test_support.rs` with the helpers listed above and add `#[cfg(test)] pub(crate) mod test_support;` plus `pub mod sources; pub mod cache;` to `src/scripture/mod.rs`. `test_support` uses `input::label_of` from Task A2.
- [ ] **Step 4: Run** both modules — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): bible_sources rows and the bounded text cache`

### Task A4: The fetch adapter

**Files:**
- Create: `src/scripture/fetch.rs`

**Interfaces:**
- Consumes: `Sources`, `status_of`, `TextCache`, `cache_cap`, `per_item_limit`, strings.
- Produces:
  - `pub type Lookup = Result<(String, Vec<VerseText>), String>;`
  - `pub trait BibleReaders: Send + Sync + 'static { fn local(&self, tr: &str, input: &str) -> Lookup; fn online<'a>(&'a self, root: &'a Path, tr: &'a str, input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>>; }` — `local` is blocking (KJV or `ImportedBible`), `online` is `bible::sources::lookup`.
  - `pub struct KjvOnly;` implementing it: `local("KJV", ..)` → `bible::kjv().lookup`; other `local` → `Err(not_available(tr))`; `online` → `Err(not_available(tr))`.
  - `pub struct Fetcher { pub readers: Arc<dyn BibleReaders>, pub cache: Arc<TextCache>, pub root: PathBuf }` with `pub async fn fetch(&self, sources: &Sources, tr: &str, input: &str) -> Lookup` dispatching by status exactly as spec §5.1's table: Offline → `local` in `tokio::task::spawn_blocking`; Online → `cache.get` hit, else `online(..).await` then `cache.put(.., cache_cap(provider_of(..)), ..)`; NeedsKey → `Err(needs_key(tr))`; Unavailable/Unknown → `Err(not_available(tr))`. Results longer than `per_item_limit(cap)` (online) or 400 (offline) → `Err(passage_too_long(limit))`.
  - **The spec's free function** (cross-plan contract, used by FEAT-6 `print/write` and FEAT-7 `service/import-pco`):
    - `pub struct FetchCtx { pub fetcher: Fetcher, pub sources: Arc<dyn Fn() -> Sources + Send + Sync> }`.
    - `pub fn install(ctx: Arc<FetchCtx>)` / `pub fn uninstall()` — a process-wide `static CTX: RwLock<Option<Arc<FetchCtx>>>` (a `RwLock`, not a `OnceLock`, so tests can install and remove it). Part B Task B2a installs the app's context at start.
    - `pub async fn fetch(root: &Path, translation: &str, input: &str) -> Lookup` — installed → `ctx.fetcher.fetch(&(ctx.sources)(), translation, input)` (`root` is ignored; the installed fetcher already owns the data root); not installed → `Fetcher::kjv_only(root).fetch(&Sources::kjv_only(), ..)`. Re-exported as `crate::scripture::fetch` (`pub use fetch::fetch;` in `mod.rs` — the module and the function live in different namespaces, so both names resolve).
    - `pub fn fetch_blocking(root: &Path, translation: &str, input: &str) -> Lookup` — for code already on the blocking pool (FEAT-7): `tokio::runtime::Handle::current().block_on(fetch(..))`; must be called from a `spawn_blocking` thread (documented on the fn).

- [ ] **Step 1: Write the failing tests** (`#[tokio::test]`; `FakeReaders` in `test_support.rs` counts calls per path, serves `"t{b}-{c}-{v}"` texts for any parseable reference, and has `fail_local(tr, msg)`, `fail_online(tr, msg)` and `set_len(n)` — return `n` synthetic verses `Psalm 119:1..` wrapping into later chapters — switches)

```rust
#[tokio::test]
async fn fetch_dispatches_by_status() {
    let (f, fake) = fetcher_with_fake();
    let src = rows(&[("KJV", "offline"), ("WEB", "offline"), ("NIV", "needs_key"), ("CSB", "unavailable"), ("ESV", "online")]);
    f.fetch(&src, "WEB", "John 3:16").await.unwrap();
    f.fetch(&src, "ESV", "John 3:16").await.unwrap();
    assert_eq!((fake.local_calls(), fake.online_calls()), (1, 1));
    assert_eq!(f.fetch(&src, "NIV", "John 3:16").await.unwrap_err(), "NIV needs a key. Add one in Settings → Bible.");
    assert_eq!(f.fetch(&src, "CSB", "John 3:16").await.unwrap_err(), "CSB isn't available.");
    assert_eq!(f.fetch(&src, "XYZ", "John 3:16").await.unwrap_err(), "XYZ isn't available.");
}
#[tokio::test]
async fn online_fetch_writes_cache_and_hits_it_next_time() {
    let (f, fake) = fetcher_with_fake();
    let src = rows(&[("ESV", "online")]);
    let a = f.fetch(&src, "ESV", "John 3:16").await.unwrap();
    let b = f.fetch(&src, "ESV", "John 3:16").await.unwrap();
    assert_eq!((fake.online_calls(), &a), (1, &b));
    assert!(f.root.join("scripture-cache/ESV.json").exists());
}
#[tokio::test]
async fn offline_text_is_never_written_to_cache() {
    let (f, fake) = fetcher_with_fake();
    let src = rows(&[("KJV", "offline")]);
    f.fetch(&src, "KJV", "John 3:16").await.unwrap();
    f.fetch(&src, "KJV", "John 3:16").await.unwrap();
    assert_eq!(fake.local_calls(), 2);
    assert!(!f.root.join("scripture-cache/KJV.json").exists());
}
#[tokio::test]
async fn kjv_only_reads_bundled_kjv() {
    let (label, vs) = Fetcher::kjv_only(tmp_root()).fetch(&Sources::kjv_only(), "KJV", "Psalm 46:10").await.unwrap();
    assert_eq!((label.as_str(), vs.len()), ("Psalm 46:10", 1));
}
#[tokio::test]
async fn online_result_over_limit_is_refused() {
    let (f, fake) = fetcher_with_fake();
    fake.set_len(401);
    let want = "That passage is too long. Choose up to about 400 verses.";
    assert_eq!(f.fetch(&rows(&[("ESV", "online")]), "ESV", "Psalm 119").await.unwrap_err(), want);
    assert_eq!(f.fetch(&rows(&[("WEB", "offline")]), "WEB", "Psalm 119").await.unwrap_err(), want);
    assert_eq!(f.cache.get("ESV", "Psalm 119"), None);   // refused text is not cached
}
#[tokio::test]
async fn free_fetch_uses_installed_context_else_kjv() {
    // one test fn, so no other test observes the installed global
    let root = tmp_root();
    assert_eq!(fetch(&root, "KJV", "John 3:16").await.unwrap().0, "John 3:16");
    assert_eq!(fetch(&root, "NIV", "John 3:16").await.unwrap_err(), "NIV isn't available.");
    let (f, fake) = fetcher_with_fake();
    install(Arc::new(FetchCtx { fetcher: f, sources: Arc::new(|| rows(&[("ESV", "online")])) }));
    fetch(&root, "ESV", "John 3:16").await.unwrap();
    assert_eq!(fake.online_calls(), 1);
    uninstall();
    assert_eq!(fetch(&root, "ESV", "John 3:16").await.unwrap_err(), "ESV isn't available.");
}
#[tokio::test(flavor = "multi_thread")]
async fn fetch_blocking_works_on_the_blocking_pool() {
    let root = tmp_root();
    let r = tokio::task::spawn_blocking(move || fetch_blocking(&root, "KJV", "Psalm 46:10")).await.unwrap();
    assert_eq!(r.unwrap().1.len(), 1);
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::fetch` — Expected: FAIL.
- [ ] **Step 3: Implement** (`Fetcher::kjv_only(root)` is a convenience constructor using `KjvOnly`). Add `pub mod fetch; pub use fetch::fetch;` to `src/scripture/mod.rs`, and `FakeReaders` / `fetcher_with_fake() -> (Fetcher, Arc<FakeReaders>)` (fetcher on a fresh `tmp_root()`) to `test_support.rs`. Only `free_fetch_uses_installed_context_else_kjv` may call `install`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): fetch adapter dispatching by bible_sources status`

### Task A5: Search query, tokenizer and snippet

**Files:**
- Create: `src/scripture/search/mod.rs`, `src/scripture/search/query.rs`

**Interfaces:**
- Produces:
  - `pub fn tokenize(text: &str) -> Vec<String>` — split on whitespace and `- – —`; per token: Unicode lowercase, remove `’` and `'`, drop every char that is not alphanumeric; drop empty tokens. Used for verses and queries alike.
  - `pub enum Term { Word(String), Phrase(Vec<String>) }`; `pub struct SearchQuery { pub terms: Vec<Term> }`.
  - `impl SearchQuery { pub fn parse(s: &str) -> Result<Self, String>; pub fn words(&self) -> Vec<&str> /* all words in order, phrases flattened */ }` — `"…"` groups a phrase (an unclosed quote runs to the end); max 12 terms (later ones ignored); empty → `Err(EMPTY_SEARCH)`. A phrase that normalises to one word becomes a `Word`.
  - `pub fn word_matches(term: &str, word: &str) -> bool` — `term.chars().count() >= 3` → `word.starts_with(term)`, else equality.
  - `pub fn snippet(text: &str, first_match_char: usize) -> String` — ≤ 160 chars of verse text; starts at a word boundary at most 40 chars before the match (or at 0); `…` prefixed/suffixed where cut. Text of ≤ 160 chars is returned whole.
  - `pub fn first_match(text: &str, q: &SearchQuery) -> usize` — char offset of the first token in `text` that matches any query word (0 if none).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn query_parses_terms_and_quoted_phrases() {
    let q = SearchQuery::parse(r#"love "be still" God"#).unwrap();
    assert_eq!(q.terms, vec![Term::Word("love".into()), Term::Phrase(vec!["be".into(), "still".into()]), Term::Word("god".into())]);
}
#[test]
fn query_normalises_case_and_apostrophes() {
    assert_eq!(SearchQuery::parse("LORD’s Don't").unwrap().words(), vec!["lords", "dont"]);
}
#[test]
fn empty_query_is_an_error() {
    for s in ["", "   ", "!!", "\"\""] { assert_eq!(SearchQuery::parse(s).unwrap_err(), "Type at least one word to search for."); }
}
#[test]
fn only_first_12_terms_count() { assert_eq!(SearchQuery::parse(&"a ".repeat(20)).unwrap().terms.len(), 12); }
#[test]
fn tokenizer_splits_dashes_and_drops_punctuation() {
    assert_eq!(tokenize("know—that I am God: the LORD's"), vec!["know", "that", "i", "am", "god", "the", "lords"]);
}
#[test]
fn prefix_rule_needs_three_letters() {
    assert!(word_matches("love", "loveth") && word_matches("an", "an") && !word_matches("an", "and"));
}
#[test]
fn snippet_trims_around_first_match() {
    let text = format!("{}needle {}", "word ".repeat(30), "tail ".repeat(40));
    let s = snippet(&text, text.find("needle").unwrap());
    assert!(s.starts_with('…') && s.ends_with('…') && s.contains("needle"));
    let body = s.trim_matches('…');
    assert!(body.chars().count() <= 160 && !body.starts_with(' '));
    assert!(body.find("needle").unwrap() <= 40);
}
#[test]
fn short_verse_snippet_is_whole() { assert_eq!(snippet("Be still, and know", 0), "Be still, and know"); }
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::search::query` — Expected: FAIL.
- [ ] **Step 3: Implement.** `search/mod.rs` declares `pub mod query; pub use query::*;` (Tasks A6–A7 add `index` and `service`).
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): search query parsing, tokenizer and snippets`

### Task A6: Search index and ranking

**Files:**
- Create: `src/scripture/search/index.rs`

**Interfaces:**
- Consumes: `tokenize`, `SearchQuery`, `word_matches`, `verse_id_of`, `BOOKS`.
- Produces:
  - `#[derive(Default)] pub enum Scope { #[default] All, Ot, Nt, Book(u8) }` whose JSON is `"all"|"ot"|"nt"|{"book":u8}` (an `#[serde(untagged)]` helper enum of a snake_case unit enum and `{book: u8}`, converted with `#[serde(from/into)]`).
  - `#[serde(rename_all = "snake_case")] pub enum Order { #[default] #[serde(other)] Best, Bible }`.
  - `pub struct SearchIndex { verses: Vec<VerseId>, refs: Vec<Box<str>>, texts: Vec<Box<str>>, norm: Vec<Box<[u32]>>, words: Vec<(Box<str>, Vec<u32>)> }`.
  - `pub fn build(read_chapter: &mut dyn FnMut(&str) -> Result<(String, Vec<VerseText>), String>, cancel: &AtomicBool) -> Option<SearchIndex>` — for each book in `BOOKS` order, chapters `"<BOOKS[b].1> <n>"` for n = 1, 2, … until the lookup fails; verses whose label doesn't parse are skipped; checks `cancel` between chapters (returns `None` when set).
  - `pub fn find(&self, q: &SearchQuery, scope: Scope) -> Vec<u32>` — ordinals in Bible order: scope as an ordinal range; each word term = union of postings over the prefix range (binary search) or exact word; phrase words intersected then confirmed by scanning `norm` for consecutive word ids that match; AND across terms.
  - `pub fn rank(&self, hits: &[u32], q: &SearchQuery, order: Order) -> Vec<u32>` — tier 1: all query words consecutive in order (prefix rule per word); tier 2: all within a window of `2 × words + 2` tokens; tier 3: the rest; sort by `(tier, ordinal)`; `Order::Bible` by ordinal only.
  - `pub struct Hit { pub id: VerseId, pub reference: String, pub snippet: String }` (Serialize); `pub fn page(&self, ranked: &[u32], q: &SearchQuery) -> (usize /*total*/, Vec<Hit>)` — first 200.
  - `pub fn chapter(&self, book: u8, chapter: u16) -> Vec<VerseText>` (context view for local translations without a second fetch).

- [ ] **Step 1: Write the failing tests.** The fixture reader `fixture()` serves text for `Genesis 1` (verses 1–4: "In the beginning God created the heaven and the earth." / "x love god" / "god love" / "god a b c d e f g h love"), `Psalms 46` (verse 10: "Be still, and know that I am God"), `John 3` (verse 16: "For God so loved the world") and `1 John 4` (verse 8: "He that loveth not knoweth not God; for God is love."). **Because `build()` walks each book from chapter 1 and stops a book at the first `Err`**, the fixture returns `Ok((label, vec![]))` for every *lower-numbered* chapter of a served book (Psalms 1–45, John 1–2, 1 John 1–3) and `Err` for the chapter after the last served one (Genesis 2, Psalms 47, John 4, 1 John 5) and for chapter 1 of every other book. Do not change `build()`'s stop rule to make the fixture pass; the rule is spec behaviour and has its own test (`build_stops_book_at_first_failed_chapter`).

```rust
#[test] fn prefix_matches_loveth_and_loved() { assert_eq!(refs(&ix(), "love", Scope::All), ["Genesis 1:2", "Genesis 1:3", "Genesis 1:4", "John 3:16", "1 John 4:8"]); }
#[test] fn short_terms_match_exactly() { assert!(refs(&ix(), "an", Scope::All).is_empty()); }
#[test] fn all_terms_must_match() { assert_eq!(refs(&ix(), "loved world", Scope::All), ["John 3:16"]); }
#[test] fn phrase_requires_consecutive_words() {
    assert_eq!(refs(&ix(), r#""god so""#, Scope::All), ["John 3:16"]);
    assert!(refs(&ix(), r#""so god""#, Scope::All).is_empty());
}
#[test] fn scope_old_testament_excludes_john() { assert!(!refs(&ix(), "god", Scope::Ot).iter().any(|r| r.contains("John"))); }
#[test] fn scope_one_book() { assert_eq!(refs(&ix(), "god", Scope::Book(18)), ["Psalm 46:10"]); }
#[test] fn scope_serde_shapes() {
    assert_eq!(serde_json::from_str::<Scope>(r#""nt""#).unwrap(), Scope::Nt);
    assert_eq!(serde_json::from_str::<Scope>(r#"{"book":18}"#).unwrap(), Scope::Book(18));
}
#[test] fn rank_tiers_phrase_then_proximity_then_rest() {
    // query "god love": Genesis 1:3 "god love" → tier 1 (consecutive, in order);
    // Genesis 1:2 "x love god" → tier 2 (wrong order, inside the 6-token window); Genesis 1:4 → tier 3 (10 tokens apart)
    assert_eq!(ranked_refs(&ix(), "god love", Order::Best)[..3], ["Genesis 1:3", "Genesis 1:2", "Genesis 1:4"]);
}
#[test] fn bible_order_option_sorts_by_ordinal() { assert_eq!(ranked_refs(&ix(), "god love", Order::Bible)[..3], ["Genesis 1:2", "Genesis 1:3", "Genesis 1:4"]); }
#[test] fn build_stops_when_cancelled() {
    assert!(build(&mut fixture(), &AtomicBool::new(true)).is_none());
    let cancel = AtomicBool::new(false);
    let mut calls = 0;
    let mut r = |c: &str| { calls += 1; if calls == 2 { cancel.store(true, Ordering::SeqCst); } fixture()(c) };
    assert!(build(&mut r, &cancel).is_none());
    assert_eq!(calls, 2);   // checked between chapters, no further reads
}
#[test] fn build_stops_book_at_first_failed_chapter() {
    // John 1–2 Err, John 3 would serve text → nothing from John is indexed
    let mut r = |c: &str| if c == "John 3" { Ok(("John 3".into(), vec![VerseText { reference: "John 3:16".into(), text: "loved".into() }])) } else { Err("no".into()) };
    let ix = build(&mut r, &AtomicBool::new(false)).unwrap();
    assert!(refs(&ix, "loved", Scope::All).is_empty());
}
#[test] fn fixture_reaches_every_served_verse() {
    assert_eq!(refs(&ix(), "god", Scope::All), ["Genesis 1:1", "Genesis 1:2", "Genesis 1:3", "Genesis 1:4", "Psalm 46:10", "John 3:16", "1 John 4:8"]);
}
#[test] fn be_still_and_know_ranks_psalm_46_10_first() {
    let ix = kjv_index();   // build() over crate::bible::kjv().lookup
    let q = SearchQuery::parse("be still and know").unwrap();
    let hits = ix.find(&q, Scope::All);
    assert_eq!(hits.len(), 3);
    let best = ix.rank(&hits, &q, Order::Best);
    assert_eq!(ix.refs_of(&best), ["Psalm 46:10", "Genesis 41:21", "1 Kings 22:3"]);
}
#[test] fn caps_at_200_and_reports_total() {
    let ix = kjv_index(); let q = SearchQuery::parse("lord").unwrap();
    let (total, page) = ix.page(&ix.rank(&ix.find(&q, Scope::All), &q, Order::Best), &q);
    assert!(total > 200); assert_eq!(page.len(), 200);
}
#[test] #[ignore] fn index_build_under_two_seconds_release() {   // run manually on the old laptop
    let t = std::time::Instant::now();
    let ix = kjv_index();
    assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(!refs(&ix, "love", Scope::All).is_empty());
}
```

  Note on the window rule: "all words within a window" means the matched positions (any order) span ≤ `2 × words + 2` tokens; tier 1 additionally needs consecutive and in query order. Genesis 1:2 is tier 2 under this definition, as asserted.

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::search::index` — Expected: FAIL.
- [ ] **Step 3: Implement.** Word ids are indices into the sorted `words` vector (built from a `BTreeMap<String, Vec<u32>>`). Test helpers `refs`, `ranked_refs`, `refs_of`, `ix()`, `kjv_index()` live in the test module (`refs_of` may be `pub(crate)` on the index).
- [ ] **Step 4: Run** — Expected: PASS (the KJV tests take about a second in release).
- [ ] **Step 5: Commit** `feat(scripture): local keyword index with prefix, phrase, scope and ranking`

### Task A7: Search service (lazy build, pre-warm, cancel, pending)

**Files:**
- Create: `src/scripture/search/service.rs`

**Interfaces:**
- Consumes: `SearchIndex::build`, `BibleReaders::local`, `Sources`, `status_of`.
- Produces:
  - `pub enum SearchAccess { Local, EsvOnline, None }`; `pub fn search_access(sources: &Sources, tr: &str, esv_search: bool) -> SearchAccess` — Offline → Local; `tr == "ESV"` and Online and `esv_search` → EsvOnline; else None. `esv_search` is `false` until the Bible session exposes ESV `/search`.
  - `pub fn search_chip(access, tr) -> String` (`SEARCH_CHIP_READY`, `SEARCH_CHIP_ESV`, `search_chip_none(tr)`).
  - `#[derive(Deserialize)] pub struct SearchRequest { query: String, translation: String, #[serde(default)] scope: Scope, #[serde(default)] order: Order }` (default scope `All`).
  - `#[derive(Serialize, Debug)] #[serde(tag = "state", rename_all = "snake_case")] pub enum SearchReply { Ready { total: usize, hits: Vec<Hit> }, Pending }`.
  - `pub struct SearchService` (`Send + Sync`, cheap to share via `Arc`): `pub fn new(rt: tokio::runtime::Handle, readers: Arc<dyn BibleReaders>, on_ready: Arc<dyn Fn(String) + Send + Sync>) -> Self`; `pub fn search(&self, sources: &Sources, req: &SearchRequest) -> Result<SearchReply, String>` — None access → `Err(search_unavailable(tr))`; parse error → `Err`; index ready → `Ready`; else start (or keep) a lazy build and return `Pending`; `pub fn prewarm(&self, sources: &Sources, tr: &str)` — builds only when access is Local, and only if nothing is building; `pub fn is_ready(&self, tr) -> bool`; `pub fn is_building(&self, tr) -> bool`.
  - **Runtime.** `search` and `prewarm` are plain sync functions that may be called from any thread, including the Slint UI thread in Part B, which has no Tokio context. So the service never calls `tokio::task::spawn_blocking` (that panics outside a runtime); it stores the `Handle` given to `new` and spawns builds with `self.rt.spawn_blocking(..)`. Part B passes the app runtime's handle.
  - Rules: at most one build at a time; starting a search for translation B while A builds (pre-warm or lazy) cancels A's build (its flag) and starts B; a cancelled translation is rebuilt lazily on its next search; builds run on the captured handle's blocking pool; on completion `on_ready(code)` is called (Part B turns it into `Update::SearchReady(code)`). Indexes are kept in memory for the service's lifetime and never written to disk.
  - EsvOnline forwarding is not built in Part A (no ESV search call exists yet); `search` returns `Err(search_unavailable("ESV"))` for it. Part B adds it only if the Bible session exposes `/search`.

- [ ] **Step 1: Write the failing tests** (`#[tokio::test(flavor = "multi_thread")]` with `Handle::current()`; `on_ready(tx)` sends the code on a `std::sync::mpsc` channel the test waits on with `recv_timeout(TEN_S)`; `noop()` ignores it; `req(query, tr)` builds a `SearchRequest`)

```rust
#[test] fn search_access_local_only_for_offline_status() {
    let src = rows(&[("KJV", "offline"), ("ESV", "online"), ("NIV", "online")]);
    assert_eq!(search_access(&src, "KJV", false), SearchAccess::Local);
    assert_eq!(search_access(&src, "ESV", false), SearchAccess::None);
    assert_eq!(search_access(&src, "ESV", true), SearchAccess::EsvOnline);
    assert_eq!(search_access(&src, "NIV", true), SearchAccess::None);
}
#[tokio::test(flavor = "multi_thread")] async fn first_search_is_pending_then_ready() {
    let (tx, rx) = mpsc::channel();
    let s = SearchService::new(Handle::current(), Arc::new(KjvOnly), on_ready(tx));
    let (src, r) = (Sources::kjv_only(), req("be still and know", "KJV"));
    assert!(matches!(s.search(&src, &r).unwrap(), SearchReply::Pending));
    assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
    match s.search(&src, &r).unwrap() {
        SearchReply::Ready { total, hits } => { assert_eq!(total, 3); assert_eq!(hits[0].reference, "Psalm 46:10"); }
        other => panic!("{other:?}"),
    }
}
#[tokio::test(flavor = "multi_thread")] async fn online_translation_never_indexed_or_prewarmed() {
    // PanicReaders: local() and online() panic for "NIV".
    let s = SearchService::new(Handle::current(), Arc::new(PanicReaders), noop());
    let src = rows(&[("NIV", "online")]);
    s.prewarm(&src, "NIV");
    assert_eq!(s.search(&src, &req("love", "NIV")).unwrap_err(), "Search isn't available for NIV. Try a translation stored on this computer, like KJV.");
    assert!(!s.is_building("NIV") && !s.is_ready("NIV"));
}
#[tokio::test(flavor = "multi_thread")] async fn prewarm_skips_online_bible_box_translation() {
    let s = SearchService::new(Handle::current(), Arc::new(PanicReaders), noop());
    s.prewarm(&rows(&[("ESV", "online")]), "ESV");
    assert!(!s.is_building("ESV") && !s.is_ready("ESV"));
}
#[tokio::test(flavor = "multi_thread")] async fn prewarm_cancels_when_other_translation_searched() {
    // SlowReaders: "WEB" local = KJV text after a 20 ms sleep per chapter; "KJV" local = bible::kjv() with no sleep.
    let (tx, rx) = mpsc::channel();
    let s = SearchService::new(Handle::current(), Arc::new(SlowReaders), on_ready(tx));
    let src = rows(&[("KJV", "offline"), ("WEB", "offline")]);
    s.prewarm(&src, "WEB");
    assert!(s.is_building("WEB"));
    assert!(matches!(s.search(&src, &req("love", "KJV")).unwrap(), SearchReply::Pending));
    assert!(!s.is_building("WEB"));                      // cancelled synchronously
    assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());   // WEB never reports ready
    assert!(matches!(s.search(&src, &req("love", "WEB")).unwrap(), SearchReply::Pending));   // lazy rebuild
}
#[tokio::test(flavor = "multi_thread")] async fn typing_while_building_stays_pending() {
    // CountingReaders: KJV text, counts local() calls for "Genesis 1".
    let (tx, rx) = mpsc::channel();
    let readers = Arc::new(CountingReaders::default());
    let s = SearchService::new(Handle::current(), readers.clone(), on_ready(tx));
    let src = Sources::kjv_only();
    assert!(matches!(s.search(&src, &req("lo", "KJV")).unwrap(), SearchReply::Pending));
    assert!(matches!(s.search(&src, &req("love", "KJV")).unwrap(), SearchReply::Pending));
    assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
    assert_eq!(readers.genesis_1_calls(), 1);   // one build
}
#[test] fn search_from_plain_thread_uses_captured_handle() {
    // no Tokio context on the calling thread, like the Slint UI thread
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let (tx, rx) = mpsc::channel();
    let s = Arc::new(SearchService::new(rt.handle().clone(), Arc::new(KjvOnly), on_ready(tx)));
    let s2 = s.clone();
    let r = std::thread::spawn(move || s2.search(&Sources::kjv_only(), &req("love", "KJV"))).join().unwrap();
    assert!(matches!(r.unwrap(), SearchReply::Pending));
    assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
}
#[test] fn search_reply_json_shapes() {
    assert_eq!(serde_json::to_value(SearchReply::Pending).unwrap(), serde_json::json!({"state":"pending"}));
    let v = serde_json::to_value(SearchReply::Ready { total: 0, hits: vec![] }).unwrap();
    assert_eq!(v, serde_json::json!({"state":"ready","total":0,"hits":[]}));
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::search::service` — Expected: FAIL.
- [ ] **Step 3: Implement.** State: `Mutex<HashMap<String, Slot>>` with `enum Slot { Building(Arc<AtomicBool>), Ready(Arc<SearchIndex>) }`, plus `rt: Handle`. The build closure reads chapters through `readers.local(tr, chapter_ref)` and runs via `self.rt.spawn_blocking`. `PanicReaders`, `SlowReaders`, `CountingReaders` live in this file's test module.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): search service with lazy local builds, pre-warm and cancel`

### Task A8: Break marks

**Files:**
- Create: `src/scripture/breaks.rs`

**Interfaces:**
- Consumes: `VerseId`, `VerseBreaks`.
- Produces:
  - `pub enum GapState { None, Auto, Added, Kept }`; `pub fn gap_state(auto_break: bool, before: bool, join: bool) -> GapState` (spec §5.4 table).
  - `pub enum Mark { Before, Join, Clear }`; `pub fn click(state: GapState) -> Mark` — None → Before; Added → Clear; Auto → Join; Kept → Clear.
  - `pub fn apply_mark(b: &mut VerseBreaks, at: VerseId, m: Mark)` — Before: insert `before`, remove `join`; Join: insert `join`, remove `before`; Clear: remove both.
  - `pub fn for_save(b: &VerseBreaks, auto_gaps: &BTreeSet<VerseId>) -> VerseBreaks` — drops `join` entries not in `auto_gaps`; a verse in both keeps only `before`.
  - `pub fn clean(b: &VerseBreaks, union: &[VerseId]) -> VerseBreaks` — drops entries not in `union`, drops `before` on `union[0]`, `before` wins over `join`.
  - `pub fn remap(b: &VerseBreaks, union: &[VerseId]) -> VerseBreaks` — `before` on a missing verse moves to the nearest existing boundary: candidates are the nearest existing id below and above; same `(book, chapter)` is preferred, then smaller verse-number distance; ties go to the later one; `join` on a missing verse is dropped; then `clean`.
  - `pub struct GapRow { pub verse: Option<VerseId>, pub state: GapState, pub enabled: bool }`; `pub fn gap_rows(union: &[Option<VerseId>], b: &VerseBreaks, auto_at: &[bool]) -> Vec<GapRow>` — one row per gap (before every verse after the first); `auto_at[i]` is true when union verse `i` starts a slide in the automatic plan (`PlannedScripture::auto_at`, Task A10). A gap before a verse without an id is `enabled: false` and its state comes from `auto_at` only (Auto or None), since no mark can be stored for it.
  - Who calls `remap`: `validate::validate_scripture` (Task A14) runs `remap` before `clean` whenever the main or second translation code changed, so a retranslated item keeps its manual breaks at the nearest boundary.

- [ ] **Step 1: Write the failing tests** (`ids`/`set` from `test_support`)

```rust
#[test] fn gap_state_table() {
    use GapState::*;
    let rows = [((false,false,false),None),((true,false,false),Auto),((false,true,false),Added),((true,true,false),Added),
                ((false,false,true),None),((true,false,true),Kept),((false,true,true),Added),((true,true,true),Added)];
    for ((a,b,j), want) in rows { assert_eq!(gap_state(a,b,j), want, "{a} {b} {j}"); }
}
#[test] fn clicks_cycle_states() {
    assert_eq!((click(GapState::None), click(GapState::Auto), click(GapState::Kept), click(GapState::Added)),
               (Mark::Before, Mark::Join, Mark::Clear, Mark::Clear));
    let at = VerseId(42, 3, 17);
    let state = |b: &VerseBreaks| gap_state(true, b.before.contains(&at), b.join.contains(&at));
    let mut b = VerseBreaks::default();
    apply_mark(&mut b, at, click(state(&b)));      // Auto → Join
    assert_eq!(state(&b), GapState::Kept);
    apply_mark(&mut b, at, click(state(&b)));      // Kept → Clear
    assert!(b.is_empty());
    assert_eq!(state(&b), GapState::Auto);
    apply_mark(&mut b, at, Mark::Before);
    assert_eq!((state(&b), b.join.len()), (GapState::Added, 0));
    apply_mark(&mut b, at, click(state(&b)));      // Added → Clear, shows Auto again
    assert_eq!(state(&b), GapState::Auto);
}
#[test] fn save_drops_stale_joins() {
    let b = VerseBreaks { before: set(&[]), join: set(&[(42,3,17),(42,3,18)]) };
    assert_eq!(for_save(&b, &set(&[(42,3,18)])).join, set(&[(42,3,18)]));
    assert!(for_save(&b, &set(&[])).join.is_empty());
}
#[test] fn before_on_first_verse_is_ignored() {
    let u = ids(&[(42,3,16),(42,3,17)]);
    let b = VerseBreaks { before: set(&[(42,3,16),(42,3,17)]), join: set(&[]) };
    assert_eq!(clean(&b, &u).before, set(&[(42,3,17)]));
}
#[test] fn verse_in_before_and_join_keeps_only_before() {
    let u = ids(&[(42,3,16),(42,3,17)]);
    let b = VerseBreaks { before: set(&[(42,3,17)]), join: set(&[(42,3,17)]) };
    let c = clean(&b, &u);
    assert_eq!((c.before, c.join), (set(&[(42,3,17)]), set(&[])));
    let s = for_save(&b, &set(&[(42,3,17)]));
    assert_eq!((s.before, s.join), (set(&[(42,3,17)]), set(&[])));
}
#[test] fn unknown_break_verses_dropped() {
    let u = ids(&[(42,3,16),(42,3,17),(42,3,18)]);
    let b = VerseBreaks { before: set(&[(42,4,1)]), join: set(&[(42,4,2)]) };
    assert!(clean(&b, &u).is_empty());
}
#[test] fn breaks_move_to_nearest_existing_boundary_on_retranslate() {
    let u = ids(&[(42,3,16),(42,3,17),(42,3,19),(42,3,20)]);
    let b = VerseBreaks { before: set(&[(42,3,18),(42,3,21)]), join: set(&[(42,3,18)]) };
    let r = remap(&b, &u);
    assert_eq!(r.before, set(&[(42,3,19),(42,3,20)]));   // 18: tie 17/19 → later; 21 → 20
    assert!(r.join.is_empty());
}
#[test] fn remap_prefers_same_chapter() {
    let u = ids(&[(42,3,35),(42,3,36),(42,4,5)]);
    let b = VerseBreaks { before: set(&[(42,4,1)]), join: set(&[]) };
    assert_eq!(remap(&b, &u).before, set(&[(42,4,5)]));   // 3:36 is closer in order but another chapter
}
#[test] fn gap_rows_disable_unparseable_verse() {
    let u = [Some(VerseId(42,3,16)), None, Some(VerseId(42,3,18))];
    let r = gap_rows(&u, &VerseBreaks::default(), &[false, true, true]);
    assert_eq!(r.len(), 2);
    assert_eq!((r[0].verse, r[0].state, r[0].enabled), (None, GapState::Auto, false));
    assert_eq!((r[1].verse, r[1].state, r[1].enabled), (Some(VerseId(42,3,18)), GapState::Auto, true));
    let r2 = gap_rows(&u, &VerseBreaks { before: set(&[(42,3,18)]), join: set(&[]) }, &[false, false, false]);
    assert_eq!((r2[0].state, r2[1].state), (GapState::None, GapState::Added));
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::breaks` — Expected: FAIL.
- [ ] **Step 3: Implement.** Derive `Debug, Clone, Copy, PartialEq, Eq` on `GapState` and `Mark`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): gap states and break marks`

### Task A9: Scripture planning — union, grouping, labels, parallel text

**Files:**
- Create: `src/scripture/planning/mod.rs`, `src/scripture/planning/measure.rs`, `src/scripture/planning/refs.rs`

**Interfaces:**
- Consumes: `verse_id_of`, `VerseBreaks`, `ParallelText`, `SlideSource`, `model::VerseText`, strings. **Not** `slides::join_refs` or `model::Theme` (both files are being rewritten by Plan 3/4 and FEAT-1; importing them would break Part A at merge).
- Produces (`refs.rs`): `pub fn join_refs(group: &[&VerseText]) -> String` — a copy of today's `slides::join_refs` (about 25 lines, `src/slides.rs` at commit `ba73280`): parse each label as `book c:v`; if any label fails, join the labels with `", "`; else runs of consecutive verses (same chapter `v+1`, or next chapter verse 1) printed as `"Book c:v–v, v"` / `"Book c:v–c:v"`. Part B Task B3 makes `slides::join_refs` delegate to this one so there is a single copy.
- Produces (`measure.rs`):
  - `pub trait PlanTheme: Clone + Hash + Send + Sync + 'static { fn scripture_size(&self) -> u32 /* 0 = default */; fn is_lower_third(&self) -> bool; }` — the only theme facts planning needs. Planning types are generic over it (`ScriptureInput<T>`, `FitSource<T>`, `CachedFits<T, F>`, `Planner<T>`), so a real `FitSource` in Part B still receives the full theme it renders with. Part B implements it for the resolved theme type (`model::Theme` before FEAT-1, FEAT-1's resolved theme after) in a new file `src/scripture/theme_impl.rs`. Tests use `test_support::TestTheme { size: u32, lower: bool }` (derives `Clone, Hash, PartialEq, Eq, Debug, Default`).
  - `pub enum Fit { Comfortable, Shrunk, TooLong }`; `pub trait Measure { fn fits(&mut self, main: &str, second: Option<&ParallelText>) -> Fit; }`.
  - `pub const MIN_PX: f32 = 26.0; pub const DEFAULT_SCRIPTURE_SIZE: u32 = 64;` `pub fn readable_px(scripture_size: u32) -> f32` = `max(0.75 × (0 → 64 else size), 40)`.
  - `pub struct CharMeasure { pub comfortable: usize, pub max: usize }`; `CharMeasure::with_limits(c, m)`; `CharMeasure::for_input<T: PlanTheme>(theme: &T, canvas: (u32, u32))`: `comfortable = round(330 × (48 / readable)² × aspect)`, `max = round(comfortable × (readable / 26)²)`, `aspect = (w / h) / (16 / 9)`. **Count** = `chars().count()` of the composed `main` slide text (superscripts and thin spaces included); with `second`, `2 × max(count(main), count(second.text))`. **Thresholds:** `count ≤ comfortable` → `Comfortable`; `comfortable < count ≤ max` → `Shrunk`; `count > max` → `TooLong`.
- Produces (`planning/mod.rs`):
  - `pub struct SecondInput { pub code: String, pub verses: Vec<VerseText>, pub notice: Option<String> }`.
  - `pub struct ScriptureInput<T: PlanTheme> { pub main: Vec<VerseText>, pub main_code: String, pub main_notice: Option<String>, pub second: Option<SecondInput>, pub per_slide: u8, pub layout: ParallelLayout, pub breaks: VerseBreaks, pub theme: T, pub canvas: (u32, u32) }` (derives `Clone, Debug`).
  - `pub struct PlannedSlide { pub label: String, pub text: String, pub reference: String, pub footer: Option<String>, pub second: Option<ParallelText>, pub source: Option<SlideSource>, pub split: bool }` (Part B converts to `Slide` with `Slide::new`, `section: None`, empty `styles`).
  - `pub struct PlannedScripture { pub slides: Vec<PlannedSlide>, pub notes: Vec<String>, pub auto_gaps: BTreeSet<VerseId>, pub auto_at: Vec<bool> }` (`PlannedSlide` and `PlannedScripture` derive `Debug, Clone, PartialEq`). `auto_gaps`/`auto_at` are filled by Task A10; in this task they are empty / all `false`.
  - `pub struct UnionVerse { pub id: Option<VerseId>, pub main: Option<VerseText>, pub second: Option<VerseText> }`; `pub fn union<T: PlanTheme>(input: &ScriptureInput<T>) -> Vec<UnionVerse>` — ordered by id; verses without an id keep their stored order right after the previous id'd verse of the same translation (at the start if none).
  - `pub fn plan_scripture<T: PlanTheme>(input: &ScriptureInput<T>, m: &mut dyn Measure) -> PlannedScripture` — spec §5.4 steps 1–5 (step 4's split is Task A10; in this task a TooLong group is emitted whole with `split: false`, and Task A10 replaces that).
  - Slide text: each verse `"{superscript(n)}\u{2009}{text}"` joined by `" "` (same as today's `scripture_slides`); a verse without an id contributes its text with no superscript. Main column from `main`, second column from `second`; a verse missing in a column contributes nothing to it. `label = refs::join_refs(union verses on the slide, using whichever VerseText exists)`; `reference = "{label} ({main_code})"` or `"{label} ({main_code} · {second_code})"`; `footer` = main notice alone, or both notices joined by `" · "` skipping blank ones, `None` when empty; `second = Some(ParallelText { main_code, second_code, text, layout })` only when `input.second` is `Some`.
  - `notes`: one `missing_verse(tr, reference)` per verse present in only one translation.
  - `source = Some(SlideSource { first, last, part: 0 })` from the first/last id'd verse on the slide; `None` when no verse has an id.

- [ ] **Step 1: Write the failing tests** (in `test_support`: `v(ref, text) -> VerseText`; `input(main: &[VerseText], second: Option<(&str, &[VerseText])>) -> ScriptureInput<TestTheme>` with `main_code "KJV"`, no notices, `per_slide 0`, SideBySide, no breaks, `TestTheme::default()`, canvas 1920×1080; `labels(&PlannedScripture) -> Vec<String>`; `cm() = CharMeasure::with_limits(60, 120)`; `short(n)` = a 10-char text)

```rust
const T: char = '\u{2009}';
#[test] fn char_measure_thresholds() {
    let mut m = CharMeasure::with_limits(60, 120);
    assert_eq!((m.fits(&"a".repeat(60), None), m.fits(&"a".repeat(61), None), m.fits(&"a".repeat(120), None), m.fits(&"a".repeat(121), None)),
               (Fit::Comfortable, Fit::Shrunk, Fit::Shrunk, Fit::TooLong));
    let p = ParallelText { main_code: "KJV".into(), second_code: "NIV".into(), text: "b".repeat(31), layout: ParallelLayout::SideBySide };
    assert_eq!(m.fits(&"a".repeat(10), Some(&p)), Fit::Shrunk);   // 2 × max(10, 31) = 62
}
#[test] fn readable_uses_resolved_scripture_size() {
    assert_eq!((readable_px(0), readable_px(64), readable_px(40), readable_px(100)), (48.0, 48.0, 40.0, 75.0));
}
#[test] fn join_refs_copy_matches_todays_output() {
    let g = |l: &[&str]| { let vs: Vec<_> = l.iter().map(|r| v(r, "x")).collect(); join_refs(&vs.iter().collect::<Vec<_>>()) };
    assert_eq!(g(&["John 3:16", "John 3:17"]), "John 3:16–17");
    assert_eq!(g(&["Psalm 23:1", "Psalm 23:4", "Psalm 23:5", "Psalm 23:6"]), "Psalm 23:1, 4–6");
    assert_eq!(g(&["John 3:36", "John 4:1", "John 4:2"]), "John 3:36–4:2");
    assert_eq!(g(&["John 3:16", "Odd label"]), "John 3:16, Odd label");
}
#[test] fn auto_groups_by_measure_not_chars() {
    let i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None);   // 43 chars each composed
    assert_eq!(plan_scripture(&i, &mut cm()).slides.len(), 2);
    assert_eq!(plan_scripture(&i, &mut CharMeasure::with_limits(200, 400)).slides.len(), 1);
}
#[test] fn manual_break_before_starts_new_slide() {
    let mut i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2)), v("John 3:18", &short(3))], None);
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–18"]);
    i.breaks.before.insert(VerseId(42, 3, 18));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–17", "John 3:18"]);
}
#[test] fn join_overrides_auto_break() {
    let mut i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None);
    i.breaks.join.insert(VerseId(42, 3, 17));
    let p = plan_scripture(&i, &mut cm());   // 87 chars: Shrunk, still one slide
    assert_eq!((labels(&p), p.slides[0].split), (vec!["John 3:16–17".to_string()], false));
}
#[test] fn per_slide_n_with_manual_break() {
    let mut i = input(&(16..=20).map(|n| v(&format!("John 3:{n}"), &short(n))).collect::<Vec<_>>(), None);
    i.per_slide = 2;
    i.breaks.before.insert(VerseId(42, 3, 17));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16", "John 3:17–18", "John 3:19–20"]);
}
#[test] fn per_slide_one_with_join_still_merges() {
    let mut i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2)), v("John 3:18", &short(3))], None);
    i.per_slide = 1;
    i.breaks.join.insert(VerseId(42, 3, 17));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–17", "John 3:18"]);
}
#[test] fn before_on_first_verse_is_ignored() {
    let mut i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2))], None);
    i.breaks.before.insert(VerseId(42, 3, 16));
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["John 3:16–17"]);
}
#[test] fn parallel_pairs_verses_by_id() {
    let i = input(&[v("John 3:16", "k16"), v("John 3:17", "k17")], Some(("NIV", &[v("John 3:17", "n17"), v("John 3:16", "n16")])));
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(200, 400));
    assert_eq!(p.slides.len(), 1);
    assert_eq!(p.slides[0].text, format!("¹⁶{T}k16 ¹⁷{T}k17"));
    let s = p.slides[0].second.as_ref().unwrap();
    assert_eq!((s.text.clone(), s.main_code.as_str(), s.second_code.as_str()), (format!("¹⁶{T}n16 ¹⁷{T}n17"), "KJV", "NIV"));
}
#[test] fn parallel_missing_verse_leaves_column_empty_and_notes_it() {
    let i = input(&[v("Acts 8:36", "k36"), v("Acts 8:37", "k37"), v("Acts 8:38", "k38")], Some(("NIV", &[v("Acts 8:36", "n36"), v("Acts 8:38", "n38")])));
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(200, 400));
    assert_eq!(p.notes, ["NIV doesn't include Acts 8:37."]);
    let s = &p.slides[0].second.as_ref().unwrap().text;
    assert!(!s.contains("³⁷") && p.slides[0].text.contains("³⁷"));
}
#[test] fn label_includes_verse_only_in_second_translation() {
    let i = input(&[v("John 3:16", "k16")], Some(("NIV", &[v("John 3:16", "n16"), v("John 3:17", "n17")])));
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(200, 400));
    assert_eq!((p.slides[0].label.as_str(), p.slides[0].reference.as_str()), ("John 3:16–17", "John 3:16–17 (KJV · NIV)"));
    assert_eq!(p.notes, ["KJV doesn't include John 3:17."]);
}
#[test] fn reference_line_names_both_translations() {
    let one = input(&[v("John 3:16", "a")], None);
    assert_eq!(plan_scripture(&one, &mut cm()).slides[0].reference, "John 3:16 (KJV)");
    let two = input(&[v("John 3:16", "a"), v("John 3:17", "b")], Some(("NIV", &[v("John 3:16", "c"), v("John 3:17", "d")])));
    assert_eq!(plan_scripture(&two, &mut CharMeasure::with_limits(200, 400)).slides[0].reference, "John 3:16–17 (KJV · NIV)");
}
#[test] fn footer_joins_non_empty_notices() {
    let mut i = input(&[v("John 3:16", "a")], Some(("NIV", &[v("John 3:16", "b")])));
    i.second.as_mut().unwrap().notice = Some("© NIV".into());
    assert_eq!(plan_scripture(&i, &mut cm()).slides[0].footer.as_deref(), Some("© NIV"));
    i.main_notice = Some("© KJV".into());
    assert_eq!(plan_scripture(&i, &mut cm()).slides[0].footer.as_deref(), Some("© KJV · © NIV"));
    i.main_notice = Some("  ".into()); i.second.as_mut().unwrap().notice = Some(String::new());
    assert_eq!(plan_scripture(&i, &mut cm()).slides[0].footer, None);
}
#[test] fn discontiguous_labels_stay_exact() {
    let i = input(&[v("Psalm 23:1", "a"), v("Psalm 23:4", "b"), v("Psalm 23:5", "c"), v("Psalm 23:6", "d")], None);
    assert_eq!(labels(&plan_scripture(&i, &mut cm())), ["Psalm 23:1, 4–6"]);
}
#[test] fn slide_source_set_for_every_slide() {
    let i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None);
    let src: Vec<_> = plan_scripture(&i, &mut cm()).slides.into_iter().map(|s| s.source).collect();
    assert_eq!(src, [Some(SlideSource { first: VerseId(42,3,16), last: VerseId(42,3,16), part: 0 }),
                     Some(SlideSource { first: VerseId(42,3,17), last: VerseId(42,3,17), part: 0 })]);
}
#[test] fn unparseable_labels_plan_in_order_without_source() {
    let mut i = input(&[v("John 3:16", "a"), v("Odd label", "b"), v("John 3:17", "c")], None);
    i.per_slide = 1;
    let p = plan_scripture(&i, &mut cm());
    assert_eq!(p.slides.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), [format!("¹⁶{T}a"), "b".to_string(), format!("¹⁷{T}c")]);
    assert_eq!(p.slides.iter().map(|s| s.source.is_some()).collect::<Vec<_>>(), [true, false, true]);
    assert_eq!(p.slides[1].label, "Odd label");
    i.per_slide = 0;
    let one = plan_scripture(&i, &mut cm());
    assert_eq!(one.slides[0].source, Some(SlideSource { first: VerseId(42,3,16), last: VerseId(42,3,17), part: 0 }));
    assert_eq!(one.slides[0].text, format!("¹⁶{T}a b ¹⁷{T}c"));
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::planning` — Expected: FAIL.
- [ ] **Step 3: Implement** the grouping loop exactly as spec §5.4 steps 1–3 (before → new group if non-empty; join → append; else `per_slide` count or `Comfortable` test of group + verse). Copy `join_refs` into `refs.rs` (do not `use crate::slides`). Add `v`, `input`, `labels`, `cm`, `short`, `TestTheme` (+ its `PlanTheme` impl) to `test_support.rs`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): measured verse grouping with marks and two translations`

### Task A10: Splitting too-long slides and automatic gaps

**Files:**
- Create: `src/scripture/planning/split.rs`
- Modify: `src/scripture/planning/mod.rs` (new file from A9)

**Interfaces:**
- Produces:
  - `pub fn split_into(text: &str, n: usize) -> Vec<String>` — `n` parts (clamped to `1..=word count`); words are separated by ASCII spaces only (so a verse superscript stays attached to its first word across the thin space `\u{2009}`); cut k sits at the boundary nearest `k × len / n` chars, preferring a sentence end (`. ? !`) within ±25 % of one share, else a clause end (`; : ,`) within ±25 %, else the nearest word boundary; never drops or reorders words; never an empty part; `parts.join(" ") == text` for text with single spaces.
  - **Splitting runs on the composed slide text** (the group's `"¹⁶\u{2009}…  ¹⁷\u{2009}…"` string, per column). So verse superscripts that fall inside a later part are kept where they are; only a later part that *begins* mid-verse starts without a superscript.
  - In `plan_scripture`: a group whose `fits` is `TooLong` is split. Let `W` = the word count of the column (single translation), or the **smaller** word count of the two non-empty columns (parallel), so both columns can always take the same `n` without an empty part. Search `n = 2..=W`: take the first `n` where every part (main part with the matching second part in parallel mode) is `Comfortable`; else the first `n` where every part is at most `Shrunk`; else `n = W` (the most parts possible). **If `W < 2`** (one unbroken token), there is nothing to split: emit the group as **one** part. In every case the loop is bounded by `W`, never panics, and keeps every word. Both translations split into the **same** `n`. Every slide of a group that was `TooLong` gets `split: true` (so the breaks dialog shows `TOO_LONG_CHIP`, including the one-part case), parts get `source.part = 0, 1, …`; labels and references of parts after the first get `" (cont.)"` after the refs (`"John 3:16–17 (cont.)"`, `"John 3:16–17 (cont.) (KJV)"`).
  - `auto_gaps` / `auto_at`: run the grouping again with `join` ignored and `before` applied, no splitting; `auto_at[i]` is true when union verse `i` (i > 0) starts a slide; `auto_gaps` holds the ids of those verses (verses without an id appear only in `auto_at`).

- [ ] **Step 1: Write the failing tests**

```rust
const T: char = '\u{2009}';
fn composed(p: &PlannedScripture) -> String { p.slides.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ") }
#[test] fn split_into_keeps_every_word_and_prefers_sentences() {
    let t = "One two three. Four five six; seven eight nine ten eleven.";
    let p = split_into(t, 2);
    assert_eq!(p.join(" "), t); assert!(p[0].ends_with('.') || p[0].ends_with(';'));
}
#[test] fn split_into_clamps_to_word_count() {
    assert_eq!(split_into("one two", 3), ["one", "two"]);
    assert_eq!(split_into("solo", 4), ["solo"]);
    assert_eq!(split_into(&format!("¹⁷{T}word next"), 2), [format!("¹⁷{T}word"), "next".to_string()]);   // superscript stays attached
}
#[test] fn join_too_long_splits_inside_verse_with_cont_label() {
    let a = "word ".repeat(14).trim_end().to_string();   // 69 chars
    let mut i = input(&[v("John 3:16", &a), v("John 3:17", &a)], None);   // composed 145 chars > 120
    i.breaks.join.insert(VerseId(42, 3, 17));
    let p = plan_scripture(&i, &mut cm());
    assert!(p.slides.len() >= 3);
    assert_eq!((p.slides[0].label.as_str(), p.slides[1].label.as_str()), ("John 3:16–17", "John 3:16–17 (cont.)"));
    assert_eq!(p.slides[1].reference, "John 3:16–17 (cont.) (KJV)");
    assert!(p.slides.iter().all(|s| s.split));
    assert_eq!(p.slides.iter().map(|s| s.source.unwrap().part).collect::<Vec<_>>(), (0..p.slides.len() as u8).collect::<Vec<_>>());
    assert_eq!(composed(&p), format!("¹⁶{T}{a} ¹⁷{T}{a}"));   // words and the ¹⁷ superscript kept in order
    assert!(p.slides.iter().all(|s| s.text.chars().count() <= 60));
}
#[test] fn parallel_long_verse_splits_both_into_same_part_count() {
    let k = "alpha ".repeat(25).trim_end().to_string();   // 149 chars
    let n = "beta ".repeat(20).trim_end().to_string();    // 99 chars
    let i = input(&[v("John 3:16", &k)], Some(("NIV", &[v("John 3:16", &n)])));
    let p = plan_scripture(&i, &mut cm());
    assert!(p.slides.len() >= 2);
    assert!(p.slides.iter().all(|s| s.split && s.second.is_some()));
    assert_eq!(composed(&p), format!("¹⁶{T}{k}"));
    let second: Vec<_> = p.slides.iter().map(|s| s.second.as_ref().unwrap().text.clone()).collect();
    assert_eq!(second.join(" "), format!("¹⁶{T}{n}"));
    assert!(second.iter().all(|t| !t.is_empty()));
}
#[test] fn single_word_too_long_group_is_one_part_and_flagged() {
    let tok = "x".repeat(500);
    let i = input(&[v("John 3:16", &tok)], None);   // composed 504 > max 400, W = 1
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(60, 400));
    assert_eq!(p.slides.len(), 1);
    assert_eq!(p.slides[0].text, format!("¹⁶{T}{tok}"));
    assert!(p.slides[0].split);                      // TooLong chip still shows
    assert_eq!(p.slides[0].label, "John 3:16");      // no "(cont.)" on part 0
}
#[test] fn two_huge_words_stop_at_word_count() {
    let i = input(&[v("John 3:16", &format!("{} {}", "x".repeat(300), "y".repeat(300)))], None);
    let p = plan_scripture(&i, &mut CharMeasure::with_limits(60, 120));
    assert_eq!(p.slides.len(), 2);   // n = W = 2, both parts still TooLong, no panic
}
#[test] fn auto_gaps_ignore_join_marks() {
    let mut i = input(&[v("John 3:16", &"a".repeat(40)), v("John 3:17", &"b".repeat(40))], None);
    i.breaks.join.insert(VerseId(42, 3, 17));
    let p = plan_scripture(&i, &mut cm());
    assert_eq!(p.slides.len(), 1);
    assert_eq!((p.auto_gaps.iter().copied().collect::<Vec<_>>(), p.auto_at.clone()), (vec![VerseId(42, 3, 17)], vec![false, true]));
}
#[test] fn scripture_slides_have_no_split_flag_when_they_fit() {
    let i = input(&[v("John 3:16", &short(1)), v("John 3:17", &short(2))], None);
    let p = plan_scripture(&i, &mut cm());
    assert!(p.slides.iter().all(|s| !s.split && s.source.unwrap().part == 0));
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::planning` — Expected: new tests FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS (A9 tests still pass).
- [ ] **Step 5: Commit** `feat(scripture): split over-long slides evenly in both translations; automatic gaps`

### Task A11: Position mapping and dialog selection helpers

**Files:**
- Create: `src/scripture/position.rs`

**Interfaces:**
- Produces:
  - `pub fn index_for_source(old: Option<&SlideSource>, old_index: usize, new: &[Option<SlideSource>]) -> usize` — the new slide whose range `first..=last` contains `old.first` with the same `part`, else part 0 of that range; with `old = None`, `old_index` clamped to `new.len() - 1`; otherwise 0 (also for empty `new`). Part B's `slides::index_for_source(old, old_index, new: &[Slide])` wraps it (the spec's signature gains `old_index`, which the "same index clamped" rule needs; FEAT-4's Refresh passes the live index).
  - `pub struct Selection { pub anchor: usize, pub head: usize }` with `pub fn at(i)`, `pub fn shift_to(&mut self, i, len)`, `pub fn extend(&mut self, delta: i32, len: usize)` (no-op past either end of the chapter), `pub fn range(&self) -> (usize, usize)` (inclusive, ordered).
  - `pub struct SeqGuard(u64)` with `pub fn next(&mut self) -> u64` and `pub fn is_current(&self, n: u64) -> bool`.
  - `pub fn add_label(chapter: &[VerseText], sel: &Selection) -> String` → `add_button(planning::refs::join_refs(..))`.

- [ ] **Step 1: Write the failing tests** (`s(first, last, part) -> SlideSource`, `src(..) = Some(s(..))`; `psalm_46()` = `verses(18, 46, 1..=11)`)

```rust
#[test] fn index_for_source_same_first_verse() {
    let new = [src((42,3,16),(42,3,17),0), src((42,3,18),(42,3,19),0), src((42,3,18),(42,3,19),1)];
    assert_eq!(index_for_source(Some(&s((42,3,18),(42,3,18),1)), 0, &new), 2);
    assert_eq!(index_for_source(Some(&s((42,3,19),(42,3,19),3)), 0, &new), 1);
}
#[test] fn index_for_source_falls_back_to_zero() {
    let new = [src((42,3,16),(42,3,17),0), src((42,3,18),(42,3,19),0)];
    assert_eq!(index_for_source(Some(&s((42,5,1),(42,5,1),0)), 1, &new), 0);
    assert_eq!(index_for_source(Some(&s((42,3,16),(42,3,16),0)), 1, &[]), 0);
}
#[test] fn index_without_source_keeps_index_clamped() {
    assert_eq!(index_for_source(None, 7, &[None, None, None]), 2);
    assert_eq!(index_for_source(None, 1, &[None, None]), 1);
    assert_eq!(index_for_source(None, 3, &[]), 0);
}
#[test] fn shift_down_stops_at_chapter_end() {
    let mut sel = Selection::at(9);
    sel.extend(1, 11); assert_eq!(sel.range(), (9, 10));
    sel.extend(1, 11); assert_eq!(sel.range(), (9, 10));   // already at the last verse: no-op
    sel.extend(-1, 11); assert_eq!(sel.range(), (9, 9));
    sel.extend(-1, 11); assert_eq!(sel.range(), (8, 9));
    let mut top = Selection::at(0);
    top.extend(-1, 5); assert_eq!(top.range(), (0, 0));
}
#[test] fn shift_click_extends_range_and_add_label_updates() {
    let ch = psalm_46();
    let mut sel = Selection::at(9);
    assert_eq!(add_label(&ch, &sel), "Add Psalm 46:10");
    sel.shift_to(10, ch.len());
    assert_eq!(add_label(&ch, &sel), "Add Psalm 46:10–11");
    sel.shift_to(7, ch.len());
    assert_eq!((sel.range(), add_label(&ch, &sel)), ((7, 9), "Add Psalm 46:8–10".to_string()));   // anchor stays at 9
}
#[test] fn seq_guard_drops_older_replies() {
    let mut g = SeqGuard::default();
    let a = g.next();
    let b = g.next();
    assert!(!g.is_current(a) && g.is_current(b));
    assert!(b > a);
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::position` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): position mapper and search selection helpers`

### Task A12a: Plan key, plan cache and the estimate fallback

**Files:**
- Create: `src/scripture/plan/mod.rs`, `src/scripture/plan/cache.rs`

**Interfaces:**
- Consumes: `ScriptureInput<T>`, `PlanTheme`, `PlannedScripture`, `plan_scripture`, `CharMeasure`, `TextCache`, `SourceStatus`.
- Produces (`plan/cache.rs`, re-exported from `plan`):
  - `#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub struct PlanKey(pub u64)`; `pub fn plan_key<T: PlanTheme>(i: &ScriptureInput<T>) -> PlanKey` — hashes every verse (reference + text) of both translations, breaks, `per_slide`, layout, codes, notices, the theme (`T: Hash`) and canvas, with `std::hash::DefaultHasher`.
  - `#[derive(Default)] pub struct PlanCache { map: RwLock<HashMap<Uuid, (PlanKey, Arc<PlannedScripture>)>> }` with `get(&self, id, key) -> Option<Arc<PlannedScripture>>` (hit only when the stored key equals `key`), `insert(&self, id, key, plan: Arc<PlannedScripture>)`, `retain(&self, ids: &HashSet<Uuid>)`, `keys(&self) -> HashMap<Uuid, PlanKey>`.
  - `pub fn plan_or_estimate<T: PlanTheme>(cache: &PlanCache, id: Uuid, input: &ScriptureInput<T>) -> (Arc<PlannedScripture>, bool /*fitting*/)` — hit → `(plan, false)`; miss → `plan_scripture(input, &mut CharMeasure::for_input(&input.theme, input.canvas))`, `(estimate, true)`; never calls any other measure.
  - `pub fn resolve_verses(stored: &[VerseText], status: SourceStatus, cache: &TextCache, tr: &str, reference: &str) -> Option<Vec<VerseText>>` — Offline → `Some(stored)`; otherwise `cache.get` (memory only); `None` = text missing (item builds no slides).
  - `pub fn changed(prev: &HashMap<Uuid, PlanKey>, now: &[(Uuid, PlanKey)]) -> Vec<Uuid>` — ids in `now` order whose key is new or different.

- [ ] **Step 1: Write the failing tests** (`sample()` = `input(&[v("John 3:16", "a"), v("John 3:17", "b")], None)` from `test_support`)

```rust
#[test] fn build_item_uses_cached_plan_when_key_matches() {
    let (c, id, i) = (PlanCache::default(), Uuid::new_v4(), sample());
    let plan = Arc::new(plan_scripture(&i, &mut CharMeasure::with_limits(10, 20)));
    c.insert(id, plan_key(&i), plan.clone());
    let (got, fitting) = plan_or_estimate(&c, id, &i);
    assert!(Arc::ptr_eq(&got, &plan) && !fitting);
}
#[test] fn build_item_falls_back_to_char_measure_and_marks_fitting() {
    let (c, id, i) = (PlanCache::default(), Uuid::new_v4(), sample());
    let want = plan_scripture(&i, &mut CharMeasure::for_input(&i.theme, i.canvas));
    let (got, fitting) = plan_or_estimate(&c, id, &i);
    assert!(fitting);
    assert_eq!(*got, want);
    let mut other = sample(); other.per_slide = 1;           // stale key in the cache is a miss too
    c.insert(id, plan_key(&other), Arc::new(plan_scripture(&other, &mut CharMeasure::with_limits(10, 20))));
    assert!(plan_or_estimate(&c, id, &i).1);
}
#[test] fn build_item_with_missing_online_text_has_no_slides() {
    let cache = TextCache::new(tmp_root());
    let stored = verses(42, 3, 16..=16);
    assert_eq!(resolve_verses(&[], SourceStatus::Online, &cache, "ESV", "John 3:16"), None);
    assert_eq!(resolve_verses(&stored, SourceStatus::Offline, &cache, "KJV", "John 3:16"), Some(stored.clone()));
    cache.put("ESV", "John 3:16", 500, "John 3:16", &stored);
    assert_eq!(resolve_verses(&[], SourceStatus::Online, &cache, "ESV", "John 3:16"), Some(stored));
}
#[test] fn plan_key_changes_with_theme_breaks_canvas_second() {
    let base = sample();
    let mut a = base.clone(); a.theme.size = 80;
    let mut b = base.clone(); b.breaks.before.insert(VerseId(42, 3, 17));
    let mut c = base.clone(); c.canvas = (1024, 768);
    let mut d = base.clone(); d.second = Some(SecondInput { code: "NIV".into(), verses: vec![v("John 3:16", "n")], notice: None });
    let keys: HashSet<_> = [&base, &a, &b, &c, &d].iter().map(|i| plan_key(i)).collect();
    assert_eq!(keys.len(), 5);
    assert_eq!(plan_key(&base), plan_key(&base.clone()));
}
#[test] fn only_changed_keys_are_resubmitted() {
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let prev = HashMap::from([(a, PlanKey(1)), (b, PlanKey(2))]);
    assert_eq!(changed(&prev, &[(a, PlanKey(1)), (b, PlanKey(3)), (c, PlanKey(1))]), vec![b, c]);
    assert!(changed(&prev, &[(a, PlanKey(1))]).is_empty());
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::plan` — Expected: FAIL.
- [ ] **Step 3: Implement.** Add `pub mod plan;` to `src/scripture/mod.rs`; `plan/mod.rs` declares `pub mod cache; pub use cache::*;` (Task A12b adds `planner`).
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): plan key, plan cache and estimate fallback`

### Task A12b: Fit cache and the coalescing planner thread

**Files:**
- Create: `src/scripture/plan/planner.rs`
- Modify: `src/scripture/plan/mod.rs` (new file from A12a: add `pub mod planner; pub use planner::*;`)

**Interfaces:**
- Consumes: `PlanCache`, `plan_key`, `plan_scripture`, `Measure`, `Fit`, `ParallelText`, `PlanTheme`, `ScriptureInput<T>`.
- Produces:
  - `pub trait FitSource<T: PlanTheme>: Send { fn fits(&mut self, theme: &T, canvas: (u32, u32), main: &str, second: Option<&ParallelText>) -> Fit; }` — Part B's `LayoutMeasure` implements it with the full theme.
  - `pub struct CachedFits<T: PlanTheme, F: FitSource<T>>` — `pub fn new(inner: F, cap: usize) -> Self` (Part B passes `FIT_LRU_CAP = 4_096`); LRU keyed by a `u64` hash of `(main, second, theme, canvas)`; on insert past `cap` evicts the least recently used. `pub fn bind<'a>(&'a mut self, theme: &'a T, canvas: (u32, u32)) -> impl Measure + 'a`. `pub fn inner(&self) -> &F` (tests).
  - `pub struct Planner<T: PlanTheme>` — `pub fn spawn(cache: Arc<PlanCache>, fits: Box<dyn FitSource<T>>, on_done: Box<dyn Fn(Vec<Uuid>) + Send>) -> Planner<T>` (thread name `"scripture-planner"`; wraps `fits` in `CachedFits` with `FIT_LRU_CAP`); `pub fn submit(&self, id: Uuid, input: ScriptureInput<T>)` — jobs coalesce per item (a pending job for the same id is replaced and keeps its queue position); the thread plans, writes `PlanCache` under `plan_key(&input)`, and calls `on_done(ids)` once per drained batch. `Drop` sets a stop flag, notifies the `Condvar` and joins the thread.

- [ ] **Step 1: Write the failing tests** (`Counting` = a `FitSource<TestTheme>` counting calls through an `Arc<AtomicUsize>` and answering `Comfortable`; `done(tx)` = an `on_done` that sends each batch on an `mpsc` channel)

```rust
#[test] fn cached_fits_hits_on_repeat_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut cf = CachedFits::new(Counting(calls.clone()), 4_096);
    let th = TestTheme::default();
    let mut m = cf.bind(&th, (1920, 1080));
    m.fits("a", None); m.fits("a", None);
    assert_eq!(calls.load(SeqCst), 1);
    for n in 0..4_096 { m.fits(&n.to_string(), None); }        // 4,097 distinct entries → "a" (oldest) evicted
    assert_eq!(calls.load(SeqCst), 4_097);
    m.fits("4095", None);                                        // still cached
    assert_eq!(calls.load(SeqCst), 4_097);
    m.fits("a", None);                                           // re-measured
    assert_eq!(calls.load(SeqCst), 4_098);
}
#[test] fn cached_fits_key_includes_theme_and_canvas() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut cf = CachedFits::new(Counting(calls.clone()), 16);
    let (t1, t2) = (TestTheme::default(), TestTheme { size: 80, lower: false });
    cf.bind(&t1, (1920, 1080)).fits("a", None);
    cf.bind(&t2, (1920, 1080)).fits("a", None);
    cf.bind(&t1, (1024, 768)).fits("a", None);
    assert_eq!(calls.load(SeqCst), 3);
}
#[test] fn planner_coalesces_jobs_per_item() {
    // GateFits: the first call sends on `entered` and blocks until `release` receives; records every `main` it sees.
    let (gate, entered, release, seen) = GateFits::new();
    let (tx, rx) = mpsc::channel();
    let cache = Arc::new(PlanCache::default());
    let p = Planner::spawn(cache.clone(), Box::new(gate), done(tx));
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    p.submit(a, input(&[v("John 3:16", "aaa")], None));
    entered.recv_timeout(TEN_S).unwrap();                     // planner is busy on A
    let bv = |t: &str| input(&[v("John 3:16", t)], None);
    p.submit(b, bv("v1text")); p.submit(b, bv("v2text")); p.submit(b, bv("v3text"));
    release.send(()).unwrap();
    let mut got = vec![];
    while !got.contains(&b) { got.extend(rx.recv_timeout(TEN_S).unwrap()); }
    assert!(cache.get(b, plan_key(&bv("v3text"))).is_some());
    let seen = seen.lock().unwrap();
    assert!(seen.iter().all(|t| !t.contains("v1text") && !t.contains("v2text")));
}
#[test] fn planner_never_runs_on_caller_thread() {
    // ThreadFits pushes std::thread::current().name() into a shared Vec on every call.
    let (fits, names) = ThreadFits::new();
    let (tx, rx) = mpsc::channel();
    let p = Planner::spawn(Arc::new(PlanCache::default()), Box::new(fits), done(tx));
    p.submit(Uuid::new_v4(), input(&[v("John 3:16", "a"), v("John 3:17", "b")], None));
    rx.recv_timeout(TEN_S).unwrap();
    let names = names.lock().unwrap();
    assert!(!names.is_empty() && names.iter().all(|n| n.as_deref() == Some("scripture-planner")));
}
#[test] fn dropping_planner_stops_the_thread() {
    let calls = Arc::new(AtomicUsize::new(0));
    let p: Planner<TestTheme> = Planner::spawn(Arc::new(PlanCache::default()), Box::new(Counting(calls.clone())), Box::new(|_| {}));
    drop(p);                                                     // joins; returns promptly
    assert_eq!(Arc::strong_count(&calls), 1);                    // the thread released its FitSource
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::plan::planner` — Expected: FAIL.
- [ ] **Step 3: Implement.** Job queue: `Mutex<(VecDeque<Uuid>, HashMap<Uuid, ScriptureInput<T>>, bool /*stop*/)>` + `Condvar`; the spec's `SyncSender<PlanJob>` is replaced by this coalescing queue (same behaviour: latest wins, one thread). The thread owns `CachedFits` and binds it per job with that job's theme and canvas. `GateFits`, `ThreadFits`, `Counting`, `done` live in this file's test module.
- [ ] **Step 4: Run** `feat-cargo test --release --lib scripture::plan` — Expected: PASS (A12a tests too).
- [ ] **Step 5: Commit** `feat(scripture): fit LRU and coalescing planner thread`

### Task A13: Two-translation rules

**Files:**
- Create: `src/scripture/parallel.rs`

**Interfaces:**
- Consumes: `Fetcher`, `Sources`, `status_of`, `is_usable`, `is_known`, `BibleSettings`, strings.
- Produces:
  - `#[derive(Deserialize)] pub struct ParallelRequest { reference, translation, second: String, #[serde(default)] second_required: bool }`.
  - `#[derive(Serialize, Debug, PartialEq)] pub struct LookupResult { label, verses: Vec<VerseText>, translation }` (same shape as `bible/lookup`'s result).
  - `#[derive(Serialize, Debug)] pub struct ParallelReply { primary: LookupResult, second: Option<LookupResult>, #[serde(skip_serializing_if = "Option::is_none")] second_error: Option<String> }`.
  - `pub async fn parallel(f: &Fetcher, sources: &Sources, req: &ParallelRequest) -> Result<ParallelReply, String>` — `second == translation` → `Err(PICK_DIFFERENT_SECOND)`; primary failure → its message; second not usable → error text = `needs_key(tr)` / `not_available(tr)` without fetching; second failure: required → `Err(couldnt_get(second, msg))`, optional → `Ok` with `second: None, second_error: Some(msg)`. Both fetches run concurrently (`tokio::join!`).
  - `#[derive(Deserialize)] pub struct BibleSettingsRequest { second_translation: Option<String>, #[serde(default)] layout: ParallelLayout }`; `pub fn check_settings_bible(req: &BibleSettingsRequest, main: &str, sources: &Sources) -> Result<BibleSettings, String>` — `None` always accepted; unknown name (`!is_known`) → `UNKNOWN_TRANSLATION`; not usable → `needs_key` / `not_available`; equal to main → `PICK_DIFFERENT_SECOND`.
  - `pub fn on_main_changed(bible: &mut BibleSettings, new_main: &str) -> bool` — turns the second off when equal; returns `second_turned_off`.
  - `pub fn planning_canvas(room_size: Option<[u32; 2]>) -> (u32, u32)` (None or a zero side → 1920×1080); `pub fn set_room_size(cur: &mut Option<[u32; 2]>, w: u32, h: u32) -> bool` (false when unchanged or zero; a zero size leaves `cur` alone).
  - `pub fn effective_layout(layout: ParallelLayout, lower_third: bool) -> ParallelLayout` — lower-third → Stacked (Part B passes `theme.is_lower_third()`).

- [ ] **Step 1: Write the failing tests** (`preq(reference, tr, second, required)` and `breq(second: Option<&str>, layout)` build requests; `fetcher_with_fake` from `test_support`)

```rust
#[tokio::test] async fn scripture_parallel_optional_second_returns_second_error() {
    let (f, fake) = fetcher_with_fake();
    fake.fail_online("ESV", "network down");
    let r = parallel(&f, &rows(&[("KJV", "offline"), ("ESV", "online")]), &preq("John 3:16", "KJV", "ESV", false)).await.unwrap();
    assert_eq!((r.primary.label.as_str(), r.primary.translation.as_str()), ("John 3:16", "KJV"));
    assert!(r.second.is_none());
    assert_eq!(r.second_error.as_deref(), Some("network down"));
    let ok = parallel(&f, &rows(&[("KJV", "offline"), ("WEB", "offline")]), &preq("John 3:16", "KJV", "WEB", false)).await.unwrap();
    assert_eq!((ok.second.unwrap().translation.as_str(), ok.second_error), ("WEB", None));
}
#[tokio::test] async fn scripture_parallel_required_second_fails_whole() {
    let (f, fake) = fetcher_with_fake();
    fake.fail_online("NIV", "timed out");
    let e = parallel(&f, &rows(&[("KJV", "offline"), ("NIV", "online")]), &preq("John 3:16", "KJV", "NIV", true)).await.unwrap_err();
    assert_eq!(e, couldnt_get("NIV", "timed out"));
    assert!(e.starts_with("Couldn't get NIV: "));
}
#[tokio::test] async fn primary_failure_fails_whole() {
    let (f, fake) = fetcher_with_fake();
    fake.fail_local("KJV", "No book called Hezekiah.");
    let e = parallel(&f, &rows(&[("KJV", "offline"), ("WEB", "offline")]), &preq("Hezekiah 1:1", "KJV", "WEB", false)).await.unwrap_err();
    assert_eq!(e, "No book called Hezekiah.");
}
#[tokio::test] async fn same_second_as_main_is_refused() {
    let (f, _) = fetcher_with_fake();
    assert_eq!(parallel(&f, &rows(&[("KJV", "offline")]), &preq("John 3:16", "KJV", "KJV", false)).await.unwrap_err(), PICK_DIFFERENT_SECOND);
}
#[tokio::test] async fn unusable_second_is_not_fetched() {
    let (f, fake) = fetcher_with_fake();
    let r = parallel(&f, &rows(&[("KJV", "offline"), ("NIV", "needs_key")]), &preq("John 3:16", "KJV", "NIV", false)).await.unwrap();
    assert_eq!((r.second_error, fake.online_calls()), (Some(needs_key("NIV")), 0));
    let e = parallel(&f, &rows(&[("KJV", "offline"), ("CSB", "unavailable")]), &preq("John 3:16", "KJV", "CSB", true)).await.unwrap_err();
    assert_eq!(e, couldnt_get("CSB", &not_available("CSB")));
}
#[test] fn settings_bible_rejects_unknown_translation() {
    assert_eq!(check_settings_bible(&breq(Some("XYZ"), ParallelLayout::SideBySide), "KJV", &rows(&[("KJV", "offline")])),
               Err(UNKNOWN_TRANSLATION.to_string()));
}
#[test] fn settings_bible_rejects_needs_key_translation() {
    let src = rows(&[("KJV", "offline"), ("NIV", "needs_key"), ("CSB", "unavailable")]);
    assert_eq!(check_settings_bible(&breq(Some("NIV"), ParallelLayout::SideBySide), "KJV", &src), Err(needs_key("NIV")));
    assert_eq!(check_settings_bible(&breq(Some("CSB"), ParallelLayout::SideBySide), "KJV", &src), Err(not_available("CSB")));
}
#[test] fn settings_bible_rejects_same_as_main() {
    assert_eq!(check_settings_bible(&breq(Some("KJV"), ParallelLayout::SideBySide), "KJV", &Sources::kjv_only()), Err(PICK_DIFFERENT_SECOND.to_string()));
}
#[test] fn settings_bible_accepts_usable_or_none() {
    let src = rows(&[("KJV", "offline"), ("WEB", "offline")]);
    assert_eq!(check_settings_bible(&breq(Some("WEB"), ParallelLayout::Stacked), "KJV", &src),
               Ok(BibleSettings { second_translation: Some("WEB".into()), layout: ParallelLayout::Stacked }));
    assert_eq!(check_settings_bible(&breq(None, ParallelLayout::SideBySide), "KJV", &src).unwrap().second_translation, None);
}
#[test] fn picking_main_equal_to_second_turns_second_off() {
    let mut b = BibleSettings { second_translation: Some("NIV".into()), layout: ParallelLayout::Stacked };
    assert!(!on_main_changed(&mut b, "KJV"));
    assert_eq!(b.second_translation.as_deref(), Some("NIV"));
    assert!(on_main_changed(&mut b, "NIV"));
    assert_eq!((b.second_translation, b.layout), (None, ParallelLayout::Stacked));
}
#[test] fn unplugged_room_keeps_last_known_canvas() {
    assert_eq!(planning_canvas(Some([1024, 768])), (1024, 768));
    assert_eq!(planning_canvas(None), (1920, 1080));
    assert_eq!(planning_canvas(Some([0, 768])), (1920, 1080));
}
#[test] fn room_size_unchanged_is_ignored() {
    let mut cur = None;
    assert!(set_room_size(&mut cur, 1920, 1080));
    assert!(!set_room_size(&mut cur, 1920, 1080));
    assert!(!set_room_size(&mut cur, 0, 1080));
    assert_eq!(cur, Some([1920, 1080]));
    assert!(set_room_size(&mut cur, 1024, 768));
}
#[test] fn stacked_forced_in_lower_third() {
    assert_eq!(effective_layout(ParallelLayout::SideBySide, true), ParallelLayout::Stacked);
    assert_eq!(effective_layout(ParallelLayout::SideBySide, false), ParallelLayout::SideBySide);
    assert_eq!(effective_layout(ParallelLayout::Stacked, false), ParallelLayout::Stacked);
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::parallel` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): two-translation lookup and settings rules`

### Task A14: Item validation and load fix-ups

**Files:**
- Create: `src/scripture/validate.rs`

**Interfaces:**
- Consumes: `ScriptureFields`, `breaks::{clean, remap}`, `verse_id_of`, `planning::refs::join_refs`, `TextCache`, `Sources`, `status_of`, `cache_cap`, `provider_of`.
- Produces:
  - `pub fn union_ids(f: &ScriptureFields) -> Vec<VerseId>` (sorted, deduped, both translations; verses without an id skipped).
  - `pub fn translations_changed(prev: &ScriptureFields, now: &ScriptureFields) -> bool` — main code differs, or second code differs (`None` vs `Some` counts).
  - `pub fn validate_scripture(f: &mut ScriptureFields, prev: Option<&ScriptureFields>, sources: &Sources, cache: &TextCache) -> Result<(), String>` — for `service/add` (`prev: None`) and `service/update` (`prev: Some(stored item before the merge)`): second == main → `Err(PICK_DIFFERENT_SECOND)` (item unchanged); then breaks: when `prev` is `Some` and `translations_changed(prev, f)` → `breaks = remap(breaks, union_ids)` (spec §5.4: a `before` mark on a verse the new translation lacks moves to the nearest boundary; `remap` ends with `clean`); otherwise `breaks = clean(breaks, union_ids)`; then `move_restricted_text`.
  - `pub fn move_restricted_text(f: &mut ScriptureFields, sources: &Sources, cache) -> bool` — main and second: `status_of(sources, tr) != Offline` and verses non-empty → `cache.put(tr, &f.reference, cache_cap(provider_of(..)), &join_refs(..), verses)`, then clear; true if anything moved. Because `status_of` uses `sources.local` when there is no row, a WEB/BSB/imported item loaded before `bible_sources` is known keeps its text.
  - `pub fn after_load_fixups<'a>(items: impl Iterator<Item = &'a mut ScriptureFields>, sources: &Sources, cache) -> bool` — per item: `move_restricted_text`; second equal to main → `None`; returns true when anything changed. (FEAT-1's `Data::after_load` calls it in Part B, passing the local translation list even before `bible_sources` arrives.)
  - `pub fn take_upgrade_toast(notes_seen: &mut BTreeSet<String>, has_scripture: bool) -> bool` — true once (inserts `UPGRADE_NOTE_KEY`), only when `has_scripture`.

- [ ] **Step 1: Write the failing tests** (`fields(tr, verses, second: Option<(&str, Vec<VerseText>)>) -> ScriptureFields` with `reference` = `join_refs` of the verses; `json(&f) = serde_json::to_string(f).unwrap()`)

```rust
#[test] fn service_update_rejects_same_second_translation() {
    let mut f = fields("KJV", verses(42, 3, 16..=17), Some(("KJV", vec![])));
    let before = json(&f);
    let cache = TextCache::new(tmp_root());
    assert_eq!(validate_scripture(&mut f, None, &Sources::kjv_only(), &cache), Err(PICK_DIFFERENT_SECOND.to_string()));
    assert_eq!(json(&f), before);
}
#[test] fn service_update_drops_unknown_break_verses() {
    let mut f = fields("KJV", verses(42, 3, 16..=18), None);
    f.breaks.before = set(&[(42,4,1), (42,3,17)]);
    let prev = f.clone();
    validate_scripture(&mut f, Some(&prev), &Sources::kjv_only(), &TextCache::new(tmp_root())).unwrap();
    assert_eq!(f.breaks.before, set(&[(42,3,17)]));
}
#[test] fn service_update_retranslate_moves_before_mark() {
    let mut prev = fields("KJV", verses(42, 3, 16..=20), None);
    prev.breaks.before = set(&[(42,3,18)]);
    let mut web: Vec<_> = verses(42, 3, 16..=20); web.retain(|v| v.reference != "John 3:18");   // WEB lacks 3:18
    let mut f = fields("WEB", web, None);
    f.breaks = prev.breaks.clone();
    let src = Sources { rows: vec![], local: vec!["KJV".into(), "WEB".into()] };
    validate_scripture(&mut f, Some(&prev), &src, &TextCache::new(tmp_root())).unwrap();
    assert_eq!(f.breaks.before, set(&[(42,3,19)]));   // tie 17/19 → later
}
#[test] fn service_update_same_translation_drops_missing_mark() {
    let mut f = fields("KJV", verses(42, 3, 16..=17), None);
    f.breaks.before = set(&[(42,3,18)]);
    let prev = f.clone();
    validate_scripture(&mut f, Some(&prev), &Sources::kjv_only(), &TextCache::new(tmp_root())).unwrap();
    assert!(f.breaks.is_empty());   // clean, not remap: nothing changed translation
}
#[test] fn service_add_moves_online_text_to_cache() {
    let cache = TextCache::new(tmp_root());
    let mut f = fields("ESV", verses(42, 3, 16..=16), None);
    validate_scripture(&mut f, None, &rows(&[("ESV", "online")]), &cache).unwrap();
    assert!(f.verses.is_empty());
    assert_eq!(cache.get("ESV", &f.reference).unwrap().1, verses(42, 3, 16..=16));
}
#[test] fn offline_text_stays_on_item() {
    let root = tmp_root();
    let mut f = fields("KJV", verses(42, 3, 16..=16), Some(("WEB", verses(42, 3, 16..=16))));
    validate_scripture(&mut f, None, &rows(&[("KJV", "offline"), ("WEB", "offline")]), &TextCache::new(root.clone())).unwrap();
    assert_eq!((f.verses.len(), f.second.as_ref().unwrap().verses.len()), (1, 1));
    assert!(!root.join("scripture-cache").exists());
}
#[test] fn online_verse_text_never_in_serialized_item() {
    let mut f = fields("ESV", vec![v("John 3:16", "ESVTEXT16")], Some(("NIV", vec![v("John 3:16", "NIVTEXT16")])));
    validate_scripture(&mut f, None, &rows(&[("ESV", "online"), ("NIV", "online")]), &TextCache::new(tmp_root())).unwrap();
    let s = json(&f);
    assert!(!s.contains("ESVTEXT16") && !s.contains("NIVTEXT16"));
}
#[test] fn after_load_moves_stored_online_text_to_cache_and_returns_true() {
    let cache = TextCache::new(tmp_root());
    let mut items = vec![fields("ESV", verses(42, 3, 16..=16), None), fields("KJV", verses(42, 3, 17..=17), None)];
    assert!(after_load_fixups(items.iter_mut(), &rows(&[("ESV", "online"), ("KJV", "offline")]), &cache));
    assert!(items[0].verses.is_empty() && items[1].verses.len() == 1);
    assert!(cache.get("ESV", &items[0].reference).is_some());
}
#[test] fn after_load_removes_second_equal_to_main() {
    let mut items = vec![fields("KJV", verses(42, 3, 16..=16), Some(("KJV", verses(42, 3, 16..=16))))];
    assert!(after_load_fixups(items.iter_mut(), &Sources::kjv_only(), &TextCache::new(tmp_root())));
    assert_eq!(items[0].second, None);
}
#[test] fn after_load_without_changes_returns_false() {
    let mut items = vec![fields("KJV", verses(42, 3, 16..=16), Some(("WEB", verses(42, 3, 16..=16))))];
    let before = json(&items[0]);
    assert!(!after_load_fixups(items.iter_mut(), &rows(&[("KJV", "offline"), ("WEB", "offline")]), &TextCache::new(tmp_root())));
    assert_eq!(json(&items[0]), before);
}
#[test] fn after_load_keeps_web_text_when_sources_unknown() {
    // bible_sources not known yet: no rows; WEB is bundled/imported, so it counts as offline
    let root = tmp_root();
    let src = Sources { rows: vec![], local: vec!["KJV".into(), "WEB".into()] };
    let mut items = vec![fields("WEB", verses(42, 3, 16..=17), None)];
    assert!(!after_load_fixups(items.iter_mut(), &src, &TextCache::new(root.clone())));
    assert_eq!(items[0].verses.len(), 2);
    assert!(!root.join("scripture-cache/WEB.json").exists());
}
#[test] fn upgrade_toast_shown_once() {
    let mut seen = BTreeSet::new();
    assert!(take_upgrade_toast(&mut seen, true));
    assert!(!take_upgrade_toast(&mut seen, true));
    assert!(seen.contains(UPGRADE_NOTE_KEY));
}
#[test] fn upgrade_toast_not_shown_without_scripture() {
    let mut seen = BTreeSet::new();
    assert!(!take_upgrade_toast(&mut seen, false));
    assert!(seen.is_empty());
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::validate` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): item validation, copyright text move and load fix-ups`

### Task A15: Parallel slide geometry

**Files:**
- Create: `src/scripture/geometry.rs`

**Interfaces:**
- Produces (pure arithmetic for Part B's `layout_parallel`; canvas pixels, scaled by `canvas_w / 1920`):
  - `pub struct Area { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }`.
  - `pub const REFERENCE_PX: f32 = 44.0; pub const HEADING_SCALE: f32 = 0.8; pub const HEADING_WEIGHT: u16 = 600; pub const HEADING_ALPHA: f32 = 0.7; pub const DIVIDER_PX: f32 = 2.0; pub const DIVIDER_ALPHA: f32 = 0.25; pub const GUTTER: f32 = 0.04; pub const FOOTER_SCALE: f32 = 0.7; pub const FOOTER_MIN_SCALE: f32 = 0.6 /* × MIN_PX */;`
  - `pub struct ParallelBoxes { pub main: Area, pub second: Area, pub divider: Area }`; `pub fn parallel_boxes(body: Area, canvas: (u32, u32), layout: ParallelLayout) -> ParallelBoxes` — SideBySide: two equal columns split by a gutter of `GUTTER × canvas_w`, divider `DIVIDER_PX × s` wide centred in it, full body height; Stacked: two equal blocks split by a gap of `GUTTER × canvas_h`, divider `DIVIDER_PX × s` tall centred in it.
  - `pub fn shared_font_px(main_fit: f32, second_fit: f32) -> f32` = the smaller (the larger block decides).
  - `pub fn footer_steps(s: f32) -> impl Iterator<Item = f32>` — from `REFERENCE_PX × FOOTER_SCALE × s` down by `2 × s` px, with the last step clamped to exactly `MIN_PX × FOOTER_MIN_SCALE × s` (s = 1: 30.8, 28.8, …, 16.8, 15.6); Part B wraps to two lines after the last step, before shrinking the body.

- [ ] **Step 1: Write the failing tests**

```rust
fn close(a: f32, b: f32) -> bool { (a - b).abs() < 0.01 }
fn inside(a: &Area, b: &Area) -> bool { a.x >= b.x - 0.01 && a.y >= b.y - 0.01 && a.x + a.w <= b.x + b.w + 0.01 && a.y + a.h <= b.y + b.h + 0.01 }
fn apart(a: &Area, b: &Area) -> bool { a.x + a.w <= b.x + 0.01 || b.x + b.w <= a.x + 0.01 || a.y + a.h <= b.y + 0.01 || b.y + b.h <= a.y + 0.01 }
#[test] fn side_by_side_columns_are_equal_and_gutter_is_4_percent() {
    let body = Area { x: 110.0, y: 100.0, w: 1700.0, h: 800.0 };
    let b = parallel_boxes(body, (1920, 1080), ParallelLayout::SideBySide);
    assert!(close(b.main.w, (1700.0 - 76.8) / 2.0) && close(b.second.w, b.main.w));
    assert!(close(b.second.x - (b.main.x + b.main.w), 76.8));
    assert!(close(b.divider.w, 2.0) && close(b.divider.h, 800.0));
    assert!(close(b.divider.x + 1.0, b.main.x + b.main.w + 38.4));   // centred in the gutter
}
#[test] fn stacked_blocks_split_height() {
    let body = Area { x: 110.0, y: 100.0, w: 1700.0, h: 800.0 };
    let b = parallel_boxes(body, (1920, 1080), ParallelLayout::Stacked);
    assert!(close(b.main.h, (800.0 - 43.2) / 2.0) && close(b.second.h, b.main.h) && close(b.main.w, 1700.0));
    assert!(close(b.second.y - (b.main.y + b.main.h), 43.2) && close(b.divider.h, 2.0));
}
#[test] fn boxes_never_overlap_and_stay_inside_body() {
    for (w, h) in [(1920, 1080), (1024, 768), (320, 180)] {
        let body = Area { x: w as f32 * 0.05, y: h as f32 * 0.05, w: w as f32 * 0.9, h: h as f32 * 0.7 };
        for l in [ParallelLayout::SideBySide, ParallelLayout::Stacked] {
            let b = parallel_boxes(body, (w, h), l);
            assert!(inside(&b.main, &body) && inside(&b.second, &body) && inside(&b.divider, &body), "{w}x{h} {l:?}");
            assert!(apart(&b.main, &b.second) && apart(&b.main, &b.divider) && apart(&b.second, &b.divider), "{w}x{h} {l:?}");
        }
    }
}
#[test] fn shared_font_px_takes_smaller() { assert_eq!((shared_font_px(48.0, 40.0), shared_font_px(30.0, 52.0)), (40.0, 30.0)); }
#[test] fn footer_steps_end_at_min() {
    let s: Vec<f32> = footer_steps(1.0).collect();
    assert!(close(s[0], 30.8) && close(*s.last().unwrap(), 15.6));
    assert!(s.windows(2).all(|w| w[0] > w[1]));
}
```
- [ ] **Step 2: Run** `feat-cargo test --release --lib scripture::geometry` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release --lib scripture` — Expected: all Part A tests PASS.
- [ ] **Step 5: Commit** `feat(scripture): parallel slide geometry`

---

# PART B — Integration after Plan 3 and Plan 4 merge

> **BLOCKED** until the `native-foundation` branch has Plans 3 and 4 merged (native operator UI, Tauri/web removed). Rebase `feat-roadmap` onto it first. Paths below follow Plan 3's layout (`src/ui/operator/*`, `src/ui/dialogs/*`, `ui/operator/*.slint`, `ui/dialogs/*.slint`); if Plan 4 renamed a file, follow the rename. Part B also needs FEAT-4's `Slide::new`, `src/strings/mod.rs`, reconcile and `Command::Refresh`, and FEAT-1's `Data::after_load` and merge-by-key; where one has not landed yet, the task says what to do instead. The Bible session's `bible_sources`, `ImportedBible` and `sources::lookup` arrive with branch `bible-esv`; until then `KjvOnly` stays the reader and only KJV is usable.
>
> Test command for Part B: `feat-cargo test --release` (whole crate, including Slint testing-backend UI tests), plus `cargo build` of the native binary.

### Task B1: Model fields, strings path and store fix-ups

**Files:**
- Modify: `src/model.rs`, `src/store.rs` (or FEAT-1's `Data::after_load` location), `src/strings/mod.rs`, `src/scripture/mod.rs`

**Interfaces:**
- `model.rs`: `pub use crate::scripture::types::{VerseId, ParallelLayout, SecondText, VerseBreaks, SlideSource, ParallelText, BibleSettings};` `ItemKind::Scripture` gains `second`, `layout`, `breaks` with the attributes from `ScriptureFields`; `Slide` gains `#[serde(default, skip_serializing_if = "Option::is_none")] source: Option<SlideSource>` (through FEAT-4's `Slide::new`); `SlideContent::Text` gains `second: Option<ParallelText>`; `Settings` gains `#[serde(default)] bible: BibleSettings`; `DisplayRoles` gains `#[serde(default)] room_size: Option<[u32; 2]>`; `Data` gains `#[serde(default)] notes_seen: BTreeSet<String>` unless FEAT-1/FEAT-4 already added it.
- `impl ItemKind { pub fn scripture_fields_mut(&mut self) -> Option<ScriptureFieldsMut<'_>> }` or convert through `ScriptureFields` (`From`/`Into` both ways) so `validate_scripture` and `after_load_fixups` run on real items.
- `Data::after_load` calls `scripture::validate::after_load_fixups` in FEAT-1's fixed order (after FEAT-2's seed, before FEAT-7). If FEAT-1 hasn't landed: call it at the end of `store::load` and save when it returns true; leave a `// FEAT-1: move into after_load` comment.
- Strings: if `src/scripture/mod.rs` still has the `#[path]` include, add `pub mod scripture;` to `src/strings/mod.rs` (creating it and `pub mod strings;` in `src/lib.rs` if no FEAT has yet — shared scaffold) and replace the `#[path]` line with `pub use crate::strings::scripture as strings;`. If A1 already used the shared path, nothing to do.
- `Data::after_load` / `after_load_fixups` receive a `Sources` whose `local` lists KJV plus every bundled/imported translation (from `ImportedBible`), even when the Bible session has not yet reported `bible_sources`.

- [ ] **Step 1: Write the failing tests:** `scripture_item_json_matches_scripture_fields` (serialize `ItemKind::Scripture` and `ScriptureFields` with the same values → identical JSON), `old_scripture_item_loads_with_defaults` (through `Data`), `settings_without_bible_section_loads`, `display_roles_without_room_size_loads`, `online_verse_text_never_in_data_json` (ESV item → save, backup, copy-item and FEAT-6 template paths → no verse text in any written file), `after_load_moves_stored_online_text_to_cache_and_returns_true`, `after_load_removes_second_equal_to_main`, `no_format_bump_for_feat5_fields` (`SUPPORTED_FORMAT` unchanged by this task).
- [ ] **Step 2: Run** `feat-cargo test --release --lib -- model:: store::` — Expected: FAIL.
- [ ] **Step 3: Implement.** Fix every `SlideContent::Text { .. }` / `Slide { .. }` construction the compiler flags (add `second: None`, `source: None`).
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(model): FEAT-5 fields on scripture items, slides, settings and displays`

### Task B2a: Scripture context and lookup/search actions

**Files:**
- Modify: `src/app.rs` (`App` fields, start-up, action table, `state_json`), `src/server.rs` (allowlist test only)
- Create: `src/scripture/readers.rs` (`SessionReaders` over `bible::kjv`, `bible::import::ImportedBible::lookup` and `bible::sources::lookup`, once `bible-esv` has merged)

**Interfaces:**
- `App` holds `scripture: Arc<ScriptureCtx>` with `fetcher: Fetcher`, `search: SearchService` (built with the app runtime's `Handle`), `plans: Arc<PlanCache>`, `planner: Planner<ResolvedTheme>` (Task B5).
- `sources() -> Sources`: rows from the Bible session's `bible_sources` (before `bible-esv`: none) and `local` = `"KJV"` + every bundled/imported translation code.
- At start the app calls `scripture::fetch::install(Arc::new(FetchCtx { fetcher, sources: <closure over App::sources> }))`, so FEAT-6's `print/write` and FEAT-7's `service/import-pco` reach the real readers through `crate::scripture::fetch(root, translation, input)` unchanged. `App::fetch(&self, tr, input)` is a thin wrapper for core code.
- Actions through `App::do_action`: `scripture/search` → `SearchService::search` (`on_ready` broadcasts `Update::SearchReady(code)`); `scripture/chapter {translation, book, chapter}` → `fetch(tr, chapter_reference(book, chapter))` (local: the index's `chapter` when built); `scripture/parallel` → `parallel::parallel`.
- None of these touch `Live`; none are added to FEAT-2's phone allowlist.

- [ ] **Step 1: Write the failing tests** (`app.rs`): `scripture_search_action_never_touches_live` (live version unchanged), `scripture_parallel_optional_second_returns_second_error`, `scripture_parallel_required_second_fails_whole`, `scripture_chapter_returns_whole_chapter`, `bible_lookup_unchanged_shape`, `free_fetch_reaches_app_readers_after_start` (a fake `SessionReaders` with WEB local: `scripture::fetch(root, "WEB", "John 3:16")` → Ok after `App` start), `remote_cannot_call_scripture_search` (server allowlist rejects every `scripture/*` and `settings/bible`).
- [ ] **Step 2: Run** `feat-cargo test --release --lib app::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): scripture context, search, chapter and parallel lookup actions`

### Task B2b: Bible settings and scripture item validation

**Files:**
- Modify: `src/app.rs` (settings and service action handlers)

**Interfaces:**
- `settings/bible` → `check_settings_bible(req, main, &sources())` then save; `settings/translation` → existing behaviour + `on_main_changed`, result `{second_turned_off}`; `displays/room_size {w, h}` → `set_room_size`, re-plan all scripture when true.
- `service/add`: per-item verse limit `per_item_limit(cache_cap(provider))` enforced; then `validate_scripture(&mut new, None, &sources(), &cache)`.
- `service/update`: merge `second`, `layout`, `breaks` by key onto the stored item (FEAT-1 §5.3: missing key keeps, `null` clears), then `validate_scripture(&mut merged, Some(&stored_before_merge), &sources(), &cache)` — **passing the previous item is what makes a translation change remap manual breaks** (spec §5.4) instead of dropping them.

- [ ] **Step 1: Write the failing tests** (`app.rs`): `service_update_rejects_same_second_translation`, `service_update_drops_unknown_break_verses`, `service_update_retranslate_moves_before_mark` (through `service/update` changing `translation` KJV → WEB where WEB lacks the marked verse → mark on the next verse), `service_update_merges_by_key_and_null_clears_second`, `service_add_over_per_item_limit_is_refused`, `settings_bible_rejects_unknown_translation`, `settings_bible_rejects_needs_key_translation`, `settings_bible_rejects_same_as_main`, `picking_main_equal_to_second_turns_second_off`, `save_settings_does_not_touch_bible` (core side of the merge rule), `room_size_change_replans_all_scripture`.
- [ ] **Step 2: Run** `feat-cargo test --release --lib app::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): Bible settings actions and scripture item validation`

### Task B3: Slides from plans, live layer and Show refusal

**Files:**
- Modify: `src/slides.rs`, `src/live.rs`, `src/app.rs` (Show path, state building)
- Create: `src/scripture/theme_impl.rs` (`impl PlanTheme for` the resolved theme type: `model::Theme` before FEAT-1, FEAT-1's resolved theme after; `scripture_size()` from the theme, `is_lower_third()` = position `"lower"`); `ResolvedTheme` is a type alias for whichever it is

**Interfaces:**
- `slides::join_refs` becomes `pub use crate::scripture::planning::refs::join_refs;` (one copy).
- `slides::build_item` for scripture: build `ScriptureInput` (resolved theme: `settings.theme` before FEAT-1, FEAT-1's resolver after; canvas `planning_canvas(displays.room_size)`; notices from `copyright_of`; text from `resolve_verses`), call `plan_or_estimate`, convert `PlannedSlide` → `Slide` via `Slide::new`. Missing text → no slides and an item state `text_missing: true`; `fitting` flag exposed in state.
- `pub use crate::scripture::position::index_for_source as index_for_source_raw;` plus `pub fn index_for_source(old: Option<&SlideSource>, old_index: usize, new: &[Slide]) -> usize` for FEAT-4's Refresh.
- Re-export `gap_state` and `plan_scripture` from `slides.rs` (spec §7 says they live there).
- `live::SlideLayer::Text` gains `#[serde(default)] second: Option<ParallelText>`; set from `SlideContent::Text.second`. `Command::Show` on an item with `text_missing` → error `text_unreachable(tr)`.
- Remove the old `scripture_slides` 330-char path once no caller remains.

- [ ] **Step 1: Write the failing tests:** `take_live_never_measures` (a `FitSource` that panics when a thread-local "in command" flag set by `App::command` is on; Show, Next, state building and the phone snapshot on a 400-verse parallel item), `next_on_400_verse_parallel_item_is_fast` (`#[ignore]` in debug; < 20 ms in release), `show_refused_for_item_with_missing_text`, `show_uses_the_given_index`, `editing_breaks_of_live_item_keeps_committed_frame`, `refresh_maps_scripture_by_index_for_source` (with FEAT-4), `scripture_slides_have_empty_styles_and_no_section`, `deleted_item_theme_replans_with_church_default` (after FEAT-1; before it `theme_change_replans`).
- [ ] **Step 2: Run** `feat-cargo test --release --lib -- slides:: live:: app::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(slides): scripture slides from measured plans; parallel slide layer`

### Task B4: Parallel rendering and the layout measure

**Files:**
- Modify: `src/render/layout.rs`, `src/render/raster.rs`, `src/render/cache.rs`, `src/render/mod.rs`
- Create: `src/render/measure.rs`

**Interfaces:**
- `layout()` → `layout_parallel()` when the slide carries `second`; geometry from `scripture::geometry` (`parallel_boxes`, `shared_font_px`, `footer_steps`), layout from `effective_layout(layout, theme.is_lower_third())`; text left-aligned; headings with translation codes; reference line unchanged; attribution footer under it with space reserved first.
- `TextLayout` gains `second_body: Option<Buffer>`, `second_box: Option<Rect>`, `headings: Option<(Buffer, Buffer)>`, `footer_box: Option<Rect>`; `raster.rs` draws them with the body's shadow rules and the divider.
- `RenderKey` gains `second` (under FEAT-1's `CacheKey::Slide(RenderKey)`).
- `measure.rs`: `pub struct LayoutMeasure { fonts: Fonts }` implementing `scripture::plan::FitSource<ResolvedTheme>` (Comfortable ≥ `readable_px`, Shrunk ≥ `MIN_PX`, else TooLong) and `pub fn fit_px(fonts, slide, theme, canvas) -> f32`.

- [ ] **Step 1: Write the failing tests:** `parallel_columns_share_one_font_size`, `stacked_forced_in_lower_third`, `parallel_text_left_aligned_in_both_layouts`, `parallel_slide_shows_both_attributions`, `attribution_shrinks_before_body`, `parallel_render_key_differs_from_single`, `layout_measure_matches_fit_px` (Comfortable/Shrunk/TooLong boundaries at 1920×1080), `lower_third_stream_view_draws_primary_only` (with FEAT-7; skip until it lands).
- [ ] **Step 2: Run** `feat-cargo test --release --lib render::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(render): side-by-side and stacked parallel scripture; layout measure`

### Task B5: Planner, pre-warm, service open and upgrade toast

**Files:**
- Modify: `src/app.rs`, `src/bin/native.rs` (pre-warm trigger after the room's first frame)

**Interfaces:**
- `App` spawns `Planner::<ResolvedTheme>::spawn(plans, Box::new(LayoutMeasure::new()), on_done)` (own `Fonts`; the planner wraps it in `CachedFits` with `FIT_LRU_CAP`); after every Data update and every text-cache fill it submits items whose `PlanKey` changed (`plan::changed`); it submits all scripture items when the canvas or resolved theme changes. `on_done` broadcasts `Update::PlansReady(ids)`; FEAT-4's reconcile runs on it (skipping items still pending).
- Service open: `TextCache::load` for every online translation in the service, `set_pinned` with the service's verse ids, then `fetch` (async) every scripture item whose text is not cached; each fill re-submits that item.
- Pre-warm: once the room commits its first frame, or 10 s after start with no room window, `search.prewarm(sources, settings.translation)`.
- Upgrade toast: on start, `take_upgrade_toast(&mut data.notes_seen, has_scripture)` → toast `UPGRADE_TOAST` once, then save.

- [ ] **Step 1: Write the failing tests:** `plans_ready_follows_data_update`, `fitting_flag_clears_when_plan_ready`, `cache_fill_replans_item`, `service_open_fetches_uncached_online_items` (fake readers), `online_item_offline_at_open_shows_missing_text`, `prewarm_waits_for_first_room_frame`, `upgrade_toast_shown_once` (through `App` start).
- [ ] **Step 2: Run** `feat-cargo test --release --lib app::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): background scripture planner, service-open fetch and search pre-warm`

### Task B6: Bible box (top bar)

**Files:**
- Modify: `ui/operator/topbar.slint`, `src/ui/operator/bible.rs`, `src/ui/operator/keys.rs` (Ctrl+Enter inside the Bible box only)

**Interfaces:**
- Submit runs `classify_bible_input`: Reference → `bible/lookup` (no second) or `scripture/parallel` with `second_required: false` (second set), then `service/add` with `second`/`layout`; BookOnly → hint row `ADD_A_CHAPTER` + `search_for_instead(text)`; UnknownWithDigits → the core error + `SEARCH_THESE_WORDS`; Words → open Find a verse (Task B7). `Ctrl+Enter` always searches. The hint row is one line, focusable with `Tab`, and clears when the text changes.
- Second chip `+ NIV` with an X (sends `settings/bible {second_translation: null}`); `+` button with "Also show…" menu (usable only; others greyed with `status_wording`); below 1500 px both fold into the chip menu as an "Also show…" sub-menu. Main-only fallback → warning toast `main_only_added`; `second_turned_off` → toast `SECOND_TURNED_OFF`. Chips greyed by `bible_sources` status.
- Factor `add_scripture_item(translation, second: Option<String>, …)` out of the submit path for Task B7.

- [ ] **Step 1: Write the failing UI tests** (testing backend, `Ops::inline`): `bible_box_words_open_search_dialog_without_adding`, `bible_box_unknown_book_with_digits_keeps_error`, `single_word_book_name_offers_search`, `words_with_digits_offer_search`, `ctrl_enter_always_searches`, `online_bible_box_translation_searches_kjv_with_note`, `second_chip_x_turns_second_off_and_saves`, `plus_menu_lists_only_usable_translations`, `second_chip_folds_into_chip_menu_below_1500px`, `bible_box_adds_parallel_item`, `unusable_second_adds_main_only_with_warning`, `alt_digit_equal_to_second_shows_turned_off_toast`.
- [ ] **Step 2: Run** `feat-cargo test --release ui::operator::bible` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): Bible box search routing and second translation chip`

### Task B7: Find a verse dialog

**Files:**
- Create: `ui/dialogs/bible-search.slint`, `src/ui/dialogs/bible_search.rs`
- Modify: `src/ui/dialogs/mod.rs` (`DialogKind::BibleSearch { query: String }`, not refreshed by `dialog_refresh`)

**Interfaces:**
- `BibleSearchDialog` properties and callbacks exactly as spec §7 "New files". Glue: 250 ms `slint::Timer` debounce (`query-submit` fires it at once), `SeqGuard` for stale replies, `Selection` for the range, `add_label` for the button, `add_scripture_item` for Add (dialog translation + church second when usable), status text from `strings` (`found`, `too_many_hits`, `index_building`, `no_hits`, `no_hits_one_word`, `searching_instead`), Best match / Bible order toggle and scope remembered for the session; `Update::SearchReady(code)` repeats the waiting query. Translation drop-down lists every `bible_sources` row, unsearchable ones greyed with `search_unavailable` tooltip. F1–F6 still clear (Plan 3 capture rule).
- **Starting translation (spec §3.1 step 9):** the drop-down starts on the Bible box translation when `search_access` for it is `Local` (or `EsvOnline`); otherwise on the first translation whose status is `Offline` (in `Sources` order, KJV when nothing else), and the status line shows `searching_instead(box_tr, local)`.
- **Esc** closes the dialog (no add, focus returns to the Bible box); F1–F6 do not close it.
- **Add limit (spec §5.7):** the context-view Add checks the selected range against `per_item_limit(cache_cap(provider_of(sources, tr)))` before sending `service/add`; over the limit → the Add button is disabled and the status line shows `passage_too_long(limit)`.

- [ ] **Step 1: Write the failing UI tests:** `search_typing_debounces_and_shows_hits`, `enter_in_search_box_focuses_first_result`, `search_down_arrow_enters_results`, `shift_click_extends_range_and_add_label_updates`, `shift_down_stops_at_chapter_end`, `search_enter_adds_after_live_and_arms`, `search_add_uses_dialog_translation_and_church_second`, `second_enter_takes_search_result_live`, `f1_in_search_dialog_clears_and_dialog_stays`, `esc_closes_search_dialog` (Esc → dialog gone, no `service/add` sent, Bible box focused), `search_dropdown_defaults_to_box_translation_when_local` (box KJV → drop-down KJV), `search_dropdown_defaults_to_first_local_when_box_is_online` (box ESV online, sources KJV offline → drop-down KJV and `searching_instead("ESV", "KJV")` shown), `context_add_over_per_item_limit_is_disabled` (fake cap 300: selecting 301 verses disables Add and shows `passage_too_long(300)`), `stale_search_reply_is_ignored`, `unsearchable_translation_greyed_in_search_dropdown`, `pending_search_repeats_on_search_ready`.
- [ ] **Step 2: Run** `feat-cargo test --release ui::dialogs::bible_search` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): Find a verse dialog`

### Task B8: Slides header, selection mapping and room size

**Files:**
- Modify: `ui/operator/slides.slint`, `src/ui/operator/slides.rs`, `src/ui/operator/` selection/keys code (Plan 3 Tasks 8, 11), room window open path (`src/ui/screens.rs` or Plan 4's equivalent)

**Interfaces:**
- Header: **Slide breaks** button for scripture items (inside the ⋯ menu below 1500 px), `FITTING_CHIP` while `fitting`, `text_unreachable(tr)` + **Try again** (re-runs `fetch` for the item) when text is missing; operator note lists `notes` (missing verses).
- When an item's slides change, the selected thumbnail and the armed slide map through `slides::index_for_source`; thumbnail clicks still send the literal index.
- Room window open → `displays/room_size {w, h}`.

- [ ] **Step 1: Write the failing UI tests:** `thumbnail_click_sends_literal_index`, `canvas_change_maps_selection_by_source`, `fitting_chip_shows_until_plan_ready`, `missing_text_item_shows_try_again`, `slide_breaks_button_only_for_scripture`, `room_window_open_sends_room_size`.
- [ ] **Step 2: Run** `feat-cargo test --release ui::operator::slides` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): scripture slide header states and selection across re-plans`

### Task B9: Item edit Bible section and Add dialog link

**Files:**
- Modify: `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs`, `ui/dialogs/add.slint`, `src/ui/dialogs/add.rs`

**Interfaces:**
- Item edit **Bible** section (FEAT-1 §7.2 sectioned, scrollable Item edit): Translation (usable only), Second translation (`None` + usable others), Layout segmented shown only with a second (Side by side disabled with `LOWER_THIRD_STACKS` when the resolved theme is lower-third), **Choose slide breaks…** (saves first, then opens Task B10). A translation change runs `scripture/parallel` with `second_required: true` (or `bible/lookup` for the main alone) off the UI thread and updates the preview; on failure the drop-down reverts and the error toasts. Save sends only changed keys; the core's `service/update` (Task B2b) remaps the item's manual breaks when the translation changed, so the dialog does not touch `breaks` itself.
- Add dialog Bible tab: **Search by words…** link opens Find a verse with the box text; Enter classifies like the Bible box.

- [ ] **Step 1: Write the failing UI tests:** `item_edit_bible_section_change_translation_relooks_up`, `item_edit_failed_lookup_reverts_dropdown`, `item_edit_layout_hidden_without_second`, `side_by_side_disabled_for_lower_third_theme`, `item_edit_save_sends_only_changed_keys`, `add_dialog_search_by_words_opens_search`.
- [ ] **Step 2: Run** `feat-cargo test --release -- ui::dialogs::item_edit ui::dialogs::add` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): Bible section in Item edit; search link in Add`

### Task B10: Slide breaks dialog

**Files:**
- Create: `ui/dialogs/scripture-breaks.slint`, `src/ui/dialogs/scripture_breaks.rs`
- Modify: `src/ui/dialogs/mod.rs` (`DialogKind::ScriptureBreaks(Uuid)`, not refreshed)

**Interfaces:**
- `ScriptureBreaksDialog` properties and callbacks as spec §7. Glue works on a clone: rows from `breaks::gap_rows` over `planning::union`; clicks through `breaks::click` + `apply_mark`; re-plan with `plan_scripture` and its own `CachedFits::new(LayoutMeasure::new(), FIT_LRU_CAP)` in `spawn_blocking` after each click (`SeqGuard` drops stale results; `busy` spinner meanwhile); thumbnails at 384 px wide through `Renderer`; `split` slides show `TOO_LONG_CHIP`; a click that creates a split shows `JOIN_TOO_LONG_NOTE`; **Reset to automatic** clears marks locally; Save sends `service/update` with `breaks: for_save(..)` and `per_slide` (changed keys only). Keys: `↑`/`↓` move between gaps, `Space` toggles.

- [ ] **Step 1: Write the failing UI tests:** `breaks_dialog_click_gap_cycles_states`, `breaks_dialog_space_toggles_focused_gap`, `breaks_dialog_reset_then_cancel_keeps_original`, `breaks_dialog_join_too_long_shows_note`, `breaks_dialog_save_drops_stale_joins`, `breaks_dialog_save_on_live_item_shows_feat4_banner`, `breaks_dialog_save_on_removed_item_shows_item_gone`, `unparseable_verse_gap_disabled_with_tooltip`.
- [ ] **Step 2: Run** `feat-cargo test --release ui::dialogs::scripture_breaks` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): slide breaks editor with live thumbnails`

### Task B11: Settings → Bible tab and Help

**Files:**
- Modify: `ui/dialogs/settings.slint` (Bible tab, Help tab), `src/ui/dialogs/settings.rs`

**Interfaces:**
- One row per `bible_sources` entry: name, `status_wording`, "Get one" link (opens `signup_url`) and a key field sending `bible/key` for `needs_key` (also shown to replace a key), `search_chip`, copyright notice. Below: Second translation drop-down and Layout segmented applied at once through `settings/bible`, with a live preview thumbnail of John 3:16–17 at the church theme; lower-third note. Save never sends `bible` (FEAT-1 merge rule). Help gains `HELP_SEARCH_RULES` and the breaks editor keys.

- [ ] **Step 1: Write the failing UI tests:** `status_rows_follow_bible_sources`, `needs_key_row_shows_get_one_and_key_field_sends_bible_key`, `save_settings_does_not_send_bible`, `second_translation_applies_at_once`, `help_lists_search_rules`.
- [ ] **Step 2: Run** `feat-cargo test --release ui::dialogs::settings` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `feat-cargo test --release` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): Settings → Bible translations, keys and second translation`

### Task B12: End-to-end checks and release notes

**Files:**
- Modify: `tests/ui_operator_flows.rs` (Plan 3 Task 22's file), release notes / `PROJECT_PLAN.md` §10b status line, `docs/superpowers/checks/` (new manual-check record)

**Interfaces:**
- One flow test per spec §3 journey: `words_to_live_psalm_46_10` (Ctrl+B, type, Enter, Enter, Enter → live on Psalm 46:10), `parallel_passage_to_live`, `manual_break_on_live_item_then_update_screen_lands_on_same_first_verse` (with FEAT-4 banner).
- Manual checks 1–7 of spec §9 recorded on the church laptop, one old laptop (Windows) and one macOS or Linux machine.
- Release notes: measured breaks may move; rollback loses second translations and manual breaks; online-translation text is not stored in `data.json`.

- [ ] **Step 1: Write the failing flow tests** listed above.
- [ ] **Step 2: Run** `feat-cargo test --release --test ui_operator_flows` — Expected: FAIL.
- [ ] **Step 3: Fix any gaps** the flows expose in Tasks B6–B10 code.
- [ ] **Step 4: Run** `feat-cargo test --release` and the ignored timing tests in release on the old laptop (`feat-cargo test --release -- --ignored index_build_under_two_seconds_release next_on_400_verse_parallel_item_is_fast`) — Expected: PASS; record times and memory in the check file.
- [ ] **Step 5: Commit** `test(feat5): end-to-end Bible extras flows; release notes`
