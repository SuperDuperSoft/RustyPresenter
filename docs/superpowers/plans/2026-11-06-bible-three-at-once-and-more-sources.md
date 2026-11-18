# Three Bibles at Once and More Bible Sources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A scripture passage shows in one to three Bibles at once (side by side or stacked), online Bible text follows publisher storage limits with a preflight warning, and the Bible library also offers Door43 and CrossWire SWORD Bibles, all behind the existing blocklist.

**Architecture:** The FEAT-5 planner already merged in `src/scripture/` changes from one `second` column to an `also` list. A new `SlideContent::Scripture` carries 1–3 `Block`s through live state, rendering, stage and phone. New library sources are new `CatalogSource` variants beside `Ebible`, so every row still goes through `CatalogRow::checked`.

**Tech Stack:** Rust 2024, Slint, cosmic-text, serde, tokio, reqwest. No new crates except where Task 12 says so.

**Spec:** `docs/superpowers/specs/2026-11-05-bible-library-all-bibles-three-at-once-design.md` (the "Nov 5 spec"). Also read Library spec `docs/superpowers/specs/2026-11-03-bible-library-design.md` §5.4, §9–§13 and FEAT-5 spec `docs/superpowers/specs/2026-11-01-feat5-bible-extras-design.md` §3.2, §5.1a, §5.4, §6. The Nov 5 spec wins where they disagree.

## What this plan does not cover

- Library Step 1 items 7–9, Step 2 (YouVersion and API.Bible catalogs) and Step 3 (original languages, versification, right-to-left text) are built from Library spec §15 directly, as the scheduled runs are doing on `bible-library-step1`. This plan replaces Library Step 4 only.
- FEAT-5 Part B's planner thread and real `LayoutMeasure`. Slides here are planned with `CharMeasure`, as today's `scripture_slides` estimates by length. Part B swaps the measure later without changing the interfaces below.

## Order and branch

- Branch `bible-three`, cut from `bible-library-step1` once Library Step 1 is finished.
- Tasks 1, 2, 10 and 11 need only Step 1. Task 3 needs Step 3's `bible/versify.rs`. Task 6 needs Step 3's `Dir` in layout (Library §9.2). If Step 3 is not merged when Task 3 starts, stop and say so; don't write a second versification table.
- Tasks run in number order unless this list allows otherwise.

## Global Constraints

- `MAX_ALSO = 2`: at most two Also Bibles, three Bibles in all.
- Every new field is `#[serde(default)]`; an old `data.json` loads unchanged. No data-format bump.
- `ParallelLayout` keeps its serde shape `"side_by_side"` / `"stacked"` and `#[serde(other)]` on `SideBySide`.
- Translations are always matched by id, never by abbreviation (Library §7.2).
- The blocklist (`bible::blocklist`) runs on every Also Bible, every catalog row and every download. NWT, JST and TPT never show anywhere.
- Text of a Bible that isn't stored on this computer is never written to `data.json`, backups or service files (Nov 5 spec §5).
- Every user-facing string is a `pub const` or small `pub fn` in `src/strings/scripture.rs` or `src/strings/catalog.rs`, worded exactly as given here.
- Exact strings: `"{abbr} turned off (same as main)."`, `"{left} isn't available right now, so only {kept} was added."` (`kept` joined with `" · "`), `"{abbr} doesn't include {reference}."` (existing `missing_verse`), `"Lower-third themes show two Bibles. {abbr} is left out."`, `"{abbr} isn't ready offline for {reference}. Open this service once with internet."`, `"{abbr} needs the internet for this passage."`, `"Used in {n} services. Those passages won't show until it's added again."`
- Layout numbers: gutter 4 % of canvas width, divider 2 px at 25 % opacity of the theme text colour, column heading `REFERENCE_PX × 0.8`, weight 600, theme colour at 70 % alpha; `MIN_PX` 26; `READABLE` unchanged.
- Bible Box and Add dialog Also controls fold into the Bible chip menu below 1500 px window width.
- Tests: in the Linux sandbox, following the recipe in `PROJECT_PLAN.md` §9 (stand-ins, `CARGO_BUILD_JOBS=1`): `cargo test --offline --lib <filter>`; before each commit, `cargo test --offline --no-fail-fast --tests` with 0 failed and no new warnings. Commit as §9's handoffs describe (patch plus temporary `GIT_INDEX_FILE`, `GIT_OPTIONAL_LOCKS=0`), since git in the OneDrive folder leaves lock files.
- Each task adds one row to `PROJECT_PLAN.md` §8 and a handoff in §12, as the scheduled runs do.

## Review Focus

1. **Main Bible right to left (Hebrew) with two left-to-right Also Bibles.** Expected: side by side puts the Hebrew column on the right and the others to its left in order; each column aligns by its own direction. Test `rtl_main_reverses_column_order` in Task 6.
2. **Two Also Bibles with the same abbreviation** (`RV1909` downloaded, `RV1909` imported). Expected: both kept, because ids differ; headings add the language code ("RV1909 · es"). Test `same_abbr_different_ids_both_kept` in Task 1.
3. **An Also Bible removed from the library while a saved service uses it.** Expected: the service loads, that column is left out, the item note says `"{abbr} needs the internet for this passage."` or the removed message, and nothing panics. Test `removed_also_bible_drops_column_with_note` in Task 4.
4. **A fixed verses-per-slide setting with three long verses in three Bibles.** Expected: text shrinks to `MIN_PX`, then every column splits into the same number of parts; text never covers the reference or credit area. Tests `three_columns_split_same_part_count` (Task 2) and `three_columns_never_overlap_footer` (Task 6).
5. **Sunday offline, one online Also Bible not cached.** Expected: the passage shows with the Bibles that have text, the item note says `"{abbr} needs the internet for this passage."`, and preflight listed it beforehand. Tests `uncached_online_also_is_dropped_with_note` (Task 9) and `preflight_flags_uncached_online_passage` (Task 9).

---

### Task 1: Data model for Also Bibles

**Files:**
- Modify: `src/model.rs` (`VerseText`, `ItemKind::Scripture`, `Settings`), `src/scripture/types.rs`
- Create: `src/scripture/also.rs`
- Test: `src/scripture/also.rs`, `src/model.rs` tests

**Interfaces:**
- Produces:
  - `model::VerseText { reference: String, text: String, #[serde(default, skip_serializing_if = "Option::is_none")] id: Option<VerseId> }`, `derive(Default)`, plus `VerseText::new(reference: impl Into<String>, text: impl Into<String>) -> VerseText`. Every existing struct literal becomes `VerseText::new(..)`. `id` replaces Library §11's separate book/chapter/verse keys.
  - `scripture::types::AlsoShow { translation: String, #[serde(default, skip_serializing_if = "Vec::is_empty")] verses: Vec<VerseText> }` (renamed from `SecondText`).
  - `ItemKind::Scripture` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] also: Vec<AlsoShow>` and `#[serde(default)] layout: ParallelLayout`. `ScriptureFields` matches it, and its `second` field is removed.
  - `Settings` gains `#[serde(default)] also_show: Vec<String>` and `#[serde(default)] parallel_layout: ParallelLayout`. `BibleSettings` is deleted.
  - `scripture::also::MAX_ALSO: usize = 2`.
  - `scripture::also::tidy(main: &str, also: &mut Vec<AlsoShow>, blocked: &dyn Fn(&str) -> bool) -> Vec<String>`: keeps order, drops entries equal to `main`, repeats of an id and blocked ids, then truncates to `MAX_ALSO`. Returns the ids dropped for being blocked, so the caller can show the blocked message.
  - `scripture::also::tidy_ids(main: &str, ids: &mut Vec<String>, blocked: &dyn Fn(&str) -> bool) -> Vec<String>`: the same rules for `Settings.also_show`.
  - `scripture::input::verse_id_of(vt: &VerseText) -> Option<VerseId>` (changed from `&str`): `vt.id` first, else the label parse it does today.

- [ ] **Step 1: Write the failing tests**

```rust
// also.rs
#[test] fn tidy_drops_main_repeats_and_blocked_then_trims() {
    let mut a = also(&["KJV", "NIV", "NIV", "TPT", "RVR", "ESV"]);
    let blocked = tidy("KJV", &mut a, &|id| id == "TPT");
    assert_eq!(ids(&a), ["NIV", "RVR"]);
    assert_eq!(blocked, ["TPT"]);
}
#[test] fn same_abbr_different_ids_both_kept() {
    let mut a = also(&["ebible:spaRV1909", "imp:RV1909"]);
    tidy("KJV", &mut a, &|_| false);
    assert_eq!(ids(&a), ["ebible:spaRV1909", "imp:RV1909"]);
}
#[test] fn tidy_ids_same_rules_for_settings() {
    let mut v = vec!["KJV".to_string(), "NIV".into(), "WEB".into(), "BSB".into()];
    tidy_ids("KJV", &mut v, &|_| false);
    assert_eq!(v, ["NIV", "WEB"]);
}
// model.rs
#[test] fn old_scripture_item_loads_with_no_also() {
    // the existing `old` fixture at model.rs:310
    assert!(matches!(&d.services[0].items[0].kind, ItemKind::Scripture { also, layout: ParallelLayout::SideBySide, .. } if also.is_empty()));
}
#[test] fn verse_id_round_trips_and_is_skipped_when_none() {
    let v = VerseText { id: Some(VerseId(42, 3, 16)), ..VerseText::new("John 3:16", "For God") };
    assert!(serde_json::to_string(&v).unwrap().contains(r#""id":[42,3,16]"#));
    assert!(!serde_json::to_string(&VerseText::new("a", "b")).unwrap().contains("id"));
}
#[test] fn settings_also_show_defaults_empty() {
    assert!(Settings::default().also_show.is_empty());
}
```

`also(&[..])` and `ids(&[..])` go in `src/scripture/test_support.rs`.

- [ ] **Step 2: Run** `cargo test --offline --lib scripture::also` and `--lib model`. Expected: FAIL (`tidy` not found).
- [ ] **Step 3: Implement** the types and functions above; replace `VerseText { .. }` literals across `src/` and `tests/` with `VerseText::new`; update FEAT-5 tests that used `SecondText`/`BibleSettings` to the new names.
- [ ] **Step 4: Run** the full test command. Expected: PASS, 0 failed.
- [ ] **Step 5: Commit** `feat(scripture): Also Bible list on scripture items and settings`.

### Task 2: Planner over a list of Also columns

**Files:**
- Modify: `src/scripture/planning/{mod.rs,measure.rs,split.rs,tests.rs}`, `src/scripture/types.rs` (`ParallelText`)

**Interfaces:**
- Consumes: `AlsoShow`, `verse_id_of(&VerseText)` (Task 1).
- Produces:
  - `ParallelText { code: String, text: String }` (drops `main_code`, `second_code`, `layout`).
  - `AlsoInput { code: String, verses: Vec<VerseText>, notice: Option<String> }` (renamed from `SecondInput`).
  - `ScriptureInput.also: Vec<AlsoInput>` (replaces `second`).
  - `Measure::fits(&mut self, main: &str, also: &[ParallelText], layout: ParallelLayout) -> Fit`. `CharMeasure` treats side by side as `main.len().max(each)` × (1 + columns × 0.5) and stacked as the sum. Those factors are the estimate only; Part B's real measure replaces them.
  - `UnionVerse { id, main: Option<VerseText>, also: Vec<Option<VerseText>> }` (len = `input.also.len()`).
  - `split::split_slide(main: &str, also: &[ParallelText], layout, m) -> Vec<(String, Vec<ParallelText>)>`: every column split into the same `n`.
  - `PlannedSlide.also: Vec<ParallelText>` (replaces `second`). Reference `"{label} (KJV · NIV · RVR)"`; footer joins non-empty notices in column order with `" · "`.
  - `missing_notes` emits one `missing_verse(code, reference)` per Bible per missing id'd verse.

- [ ] **Step 1: Write the failing tests** (rename the existing parallel tests to the list form, keep their assertions, then add):

```rust
#[test] fn three_bibles_pair_verses_by_id() {
    let p = plan(input3(&["John 3:16", "John 3:17"], &["John 3:16", "John 3:17"], &["John 3:16", "John 3:17"]));
    assert_eq!(p.slides[0].also.len(), 2);
    assert!(p.slides[0].also[1].text.contains("¹⁶"));
}
#[test] fn verse_missing_in_two_bibles_notes_each() {
    let p = plan(input3(&["Acts 8:36", "Acts 8:37"], &["Acts 8:36"], &["Acts 8:36"]));
    assert_eq!(p.notes, ["NIV doesn't include Acts 8:37.", "RVR doesn't include Acts 8:37."]);
}
#[test] fn three_columns_split_same_part_count() {
    let mut i = input3_long(1); i.per_slide = 1;
    let p = plan(i);
    assert!(p.slides.len() > 1);
    assert!(p.slides.iter().all(|s| s.also.len() == 2 && s.also.iter().all(|c| !c.text.is_empty())));
}
#[test] fn reference_line_names_three_bibles() {
    assert_eq!(plan(input3(&["John 3:16"], &["John 3:16"], &["John 3:16"])).slides[0].reference, "John 3:16 (KJV · NIV · RVR)");
}
#[test] fn verse_id_field_beats_label() {
    let mut i = input3(&["Juan 3:16"], &[], &[]); i.main[0].id = Some(VerseId(42, 3, 16));
    assert_eq!(plan(i).slides[0].source.unwrap().first, VerseId(42, 3, 16));
}
```

- [ ] **Step 2: Run** `cargo test --offline --lib scripture::planning`. Expected: FAIL to compile on `also`.
- [ ] **Step 3: Implement** the interface changes. `union` loops over `[main] + also` with the column index in place of `is_second`.
- [ ] **Step 4: Run** the full test command. Expected: PASS.
- [ ] **Step 5: Commit** `feat(scripture): plan slides for up to three Bibles`.

### Task 3: Lookup with Also Bibles

**Files:**
- Create: `src/scripture/align.rs`
- Modify: `src/app.rs` (`"bible/lookup"` at about line 707), `src/strings/scripture.rs`

**Interfaces:**
- Consumes: Library Step 3's `bible/versify.rs` (whatever names it exports to map a verse to and from English numbering), `BibleMeta` (`versification`), `AlsoShow`.
- Produces:
  - `scripture::align::align(main: &[VerseText], main_vsys, other: &[VerseText], other_vsys) -> Vec<VerseText>`: one entry per main verse with an id, mapped through English; a verse the other Bible lacks gives `VerseText { text: "", id: Some(mapped) }`. Verses only the other Bible has are appended in id order. This is the only caller of versify for parallel work.
  - `bible/lookup` request gains `#[serde(default)] also: Vec<String>`. Reply: `{"verses": [...], "also": [{"translation": id, "verses": [...]}], "left_out": [ids]}`. A lookup error for an Also Bible puts its id in `left_out`; it never fails the whole request. Every verse carries `id`.

- [ ] **Step 1: Write the failing tests**

```rust
// align.rs
#[test] fn septuagint_psalm_maps_through_english() {
    // English Ps 23:1 is LXX Ps 22:1
    let out = align(&[v_id("Psalm 23:1", (18, 23, 1))], Vsys::English, &[v_id("Psalm 22:1", (18, 22, 1))], Vsys::Septuagint);
    assert_eq!(out[0].id, Some(VerseId(18, 22, 1)));
    assert!(!out[0].text.is_empty());
}
#[test] fn missing_verse_is_empty_not_shifted() {
    let out = align(&[v_id("Acts 8:36", (43, 8, 36)), v_id("Acts 8:37", (43, 8, 37))], Vsys::English, &[v_id("Acts 8:36", (43, 8, 36))], Vsys::English);
    assert_eq!((out[1].id, out[1].text.as_str()), (Some(VerseId(43, 8, 37)), ""));
}
// app.rs
#[tokio::test] async fn lookup_with_also_returns_each_and_left_out() {
    let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"KJV","also":["WEB","ESV"]}"#).await.unwrap();
    assert_eq!(v["also"][0]["translation"], "WEB");
    assert_eq!(v["left_out"], json!(["ESV"])); // no key saved
}
```

- [ ] **Step 2: Run** `cargo test --offline --lib scripture::align` and `--lib app::tests::lookup_with_also`. Expected: FAIL.
- [ ] **Step 3: Implement.** Look up each Also Bible concurrently with `futures_util::future::join_all` (`futures-util` is in `Cargo.toml`).
- [ ] **Step 4: Run** the full test command. Expected: PASS.
- [ ] **Step 5: Commit** `feat(bible): look up a passage in up to three Bibles, verse-aligned`.

### Task 4: Item and settings rules in the app

**Files:**
- Modify: `src/app.rs` (`service/add`, `service/update`, `settings/translation`, load), `src/strings/scripture.rs`

**Interfaces:**
- Consumes: `tidy`, `tidy_ids` (Task 1), `bible/lookup` (Task 3).
- Produces:
  - Action `settings/also` `{also_show: [ids], parallel_layout}` → `{}`; runs `tidy_ids` against `settings.translation`.
  - `settings/translation`: if the new main is in `also_show`, remove it and return toast `"{abbr} turned off (same as main)."`.
  - `service/add`: when `also_show` is non-empty and the item has no `also`, the core looks the Also Bibles up (Task 3); anything in `left_out` gives toast `"{left} isn't available right now, so only {kept} was added."`.
  - Load (`after_load`): `tidy` on every scripture item; an Also id with no `BibleMeta` and no source is kept in data but shown with note `"That Bible was removed. Add it again in Settings → Bibles."` (Library §13) and its column left out.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test] async fn main_equal_to_also_turns_also_off() {
    app.do_action("settings/also", br#"{"also_show":["WEB"],"parallel_layout":"stacked"}"#).await.unwrap();
    let r = app.do_action("settings/translation", br#"{"translation":"WEB"}"#).await.unwrap();
    assert_eq!(r["toast"], "WEB turned off (same as main).");
    assert!(app.data().settings.also_show.is_empty());
}
#[tokio::test] async fn add_with_unavailable_also_adds_the_rest_and_warns() {
    // also_show = ["WEB", "ESV"], no ESV key
    assert_eq!(r["toast"], "ESV isn't available right now, so only KJV · WEB was added.");
    assert_eq!(also_ids(&item), ["WEB"]);
}
#[test] fn removed_also_bible_drops_column_with_note() {
    // item also = ["ebible:spaRV1909"], no such Bible installed
    let b = build_scripture(&data, &item);
    let (notes, slides) = (b.notes, b.slides);
    assert!(slides.iter().all(|s| scripture_blocks(s).len() == 1));
    assert!(notes.contains(&"That Bible was removed. Add it again in Settings → Bibles.".to_string()));
}
#[tokio::test] async fn blocked_also_is_refused_everywhere() {
    let err = app.do_action("settings/also", br#"{"also_show":["TPT"]}"#).await.unwrap_err();
    assert_eq!(err.message, "This translation can't be used in RustyPresenter. Please choose another Bible.");
}
```

`build_scripture` comes from Task 5; write this test now and expect it to fail until then if Task 5 is not done.

- [ ] **Step 2–4:** FAIL → implement → full test command PASS.
- [ ] **Step 5: Commit** `feat(app): Also Bible defaults, same-as-main and unavailable rules`.

### Task 5: Scripture slides carry blocks

**Files:**
- Modify: `src/model.rs` (`SlideContent`), `src/slides.rs` (`scripture_slides`, `build_item` at about line 215), `src/live.rs` (`SlideLayer`), `src/render/cache.rs` (`RenderKey`)

**Interfaces:**
- Consumes: `plan_scripture`, `CharMeasure` (Task 2), `BibleMeta` (`dir`, `lang`, credit).
- Produces:
  - `model::Block { text: String, code: String, #[serde(default)] dir: Dir, #[serde(default)] lang: String, #[serde(default)] credit: Option<String> }`.
  - `SlideContent::Scripture { blocks: Vec<Block>, layout: ParallelLayout, reference: Option<String>, #[serde(default)] size: u32 }`, 1–3 blocks, main first. Single-Bible scripture also uses this variant; songs keep `Text`.
  - `SlideLayer::Text` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] also: Vec<Block>` and `layout: ParallelLayout`; `text` stays the main Bible so stage and remote keep working until Task 7. `RenderKey` gains the same two fields.
  - `slides::ScriptureBuild { slides: Vec<Slide>, notes: Vec<String> }` and `slides::build_scripture(data: &Data, item: &ServiceItem) -> ScriptureBuild`. `build_item` (`-> (String, &'static str, Vec<Slide>)`, unchanged) calls it and keeps `slides`; the item view and preflight read `notes` from `build_scripture`.
  - Credit: `block.credit` is set on the first slide of the passage only (Nov 5 spec §3, Library §10), following `settings.credits`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn three_bible_item_builds_three_blocks() {
    let s = &slides_of(item3("John 3:16-17"))[0];
    assert!(matches!(&s.content, SlideContent::Scripture { blocks, .. } if blocks.iter().map(|b| b.code.as_str()).eq(["KJV", "WEB", "BSB"])));
}
#[test] fn credit_only_on_first_slide() {
    let s = slides_of(item3_long());
    assert!(blocks(&s[0]).iter().any(|b| b.credit.is_some()));
    assert!(s[1..].iter().all(|x| blocks(x).iter().all(|b| b.credit.is_none())));
}
#[test] fn single_bible_still_counts_like_before() {
    // the existing per_slide assertions at slides.rs:288-290, through the new function
    assert_eq!(slides_of(kjv_item(5, 2)).len(), 3);
}
```

- [ ] **Step 2–4:** FAIL → implement (update every `match` on `SlideContent`; the compiler lists them) → full test command PASS, including `render_golden` unchanged for single-Bible slides.
- [ ] **Step 5: Commit** `feat(slides): scripture slides carry one block per Bible`.

### Task 6: Draw two and three Bibles

**Files:**
- Create: `src/render/parallel.rs`
- Modify: `src/render/layout.rs` (`layout()` dispatches when `also` is non-empty), `src/render/raster.rs`, `tests/render_golden.rs` and its goldens

**Interfaces:**
- Consumes: `SlideLayer::Text { also, layout }` (Task 5), `Dir`, fonts per `lang` (Library §9).
- Produces: `render::parallel::layout_parallel(fonts: &mut Fonts, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> TextLayout`, with `TextLayout` gaining `columns: Vec<Column>` where `Column { heading: Buffer, body: Buffer, credit: Option<Buffer>, rect: Rect, dir: Dir }` and `dividers: Vec<Rect>`.

Rules (Nov 5 spec §3): equal columns or rows; one font size, shrunk until every block fits; reference line and credits reserved first; side by side reverses column order when the main block is right to left; lower-third themes stack the main and first Also block only.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn three_columns_equal_width_with_two_dividers() {
    let l = lay(three(), ParallelLayout::SideBySide);
    assert_eq!((l.columns.len(), l.dividers.len()), (3, 2));
    let w: Vec<_> = l.columns.iter().map(|c| c.rect.w).collect();
    assert!(w.iter().all(|x| (x - w[0]).abs() < 1.0));
}
#[test] fn rtl_main_reverses_column_order() {
    let l = lay(hebrew_then_two_ltr(), ParallelLayout::SideBySide);
    assert!(l.columns[0].rect.x > l.columns[2].rect.x);
    assert_eq!(l.columns[0].dir, Dir::Rtl);
}
#[test] fn one_size_for_all_columns() { /* every column body's font size equal */ }
#[test] fn three_columns_never_overlap_footer() {
    let l = lay(three_long_at_min_px(), ParallelLayout::SideBySide);
    assert!(l.columns.iter().all(|c| c.rect.bottom() <= l.reference_box.y));
}
#[test] fn lower_third_draws_two_of_three() {
    assert_eq!(lay_lower_third(three()).columns.len(), 2);
}
```

Goldens (new, by eye on first generation): `parallel_three_side.png`, `parallel_three_stacked.png`, `parallel_en_es_he.png`, `parallel_two_side.png`.

- [ ] **Step 2: Run** `cargo test --offline --lib render::parallel`. Expected: FAIL.
- [ ] **Step 3: Implement.** The lower-third note `"Lower-third themes show two Bibles. {abbr} is left out."` is added by `scripture_slides` (Task 5's notes), not the renderer.
- [ ] **Step 4: Run** `UPDATE_GOLDENS=1 cargo test --offline --test render_golden`, open each new PNG and check it against the rules above, then the full test command. Expected: PASS.
- [ ] **Step 5: Commit** `feat(render): side-by-side and stacked layouts for up to three Bibles`.

### Task 7: Stage and phone show every Bible

**Files:**
- Modify: `ui/stage.slint` and its Rust model, `src/server.rs`, `web/remote.js`

**Interfaces:**
- Consumes: `SlideLayer::Text { also }` (Task 5).
- Produces: phone slide payload `blocks: [{text, dir, lang}]` (main first), replacing Library §12's `text2`, `dir2`, `lang2`. Stage shows the blocks stacked, each with its code.

- [ ] **Step 1: Write the failing tests:** `remote_payload_has_three_blocks` (server JSON has `blocks.len() == 3`, `blocks[2].dir == "rtl"` for Hebrew); `stage_model_stacks_blocks` (Slint testing backend: three text elements with ids `stage-block-0..2`).
- [ ] **Step 2–4:** FAIL → implement → full test command PASS.
- [ ] **Step 5: Commit** `feat(stage, phone): show every Bible of a parallel passage`.

### Task 8: Also Show controls

**Files:**
- Modify: `ui/operator/topbar.slint` (Bible box), `ui/dialogs/add.slint`, `ui/dialogs/item-edit.slint`, `ui/dialogs/settings-bible.slint`, their Rust view code, `tests/ui_operator_flows.rs`

**Interfaces:**
- Consumes: the Library §7.2 picker from Step 1 item 7; actions `settings/also` (Task 4) and `service/update` with `also` and `layout`.
- Produces: "Also show: [none ▾]" using the picker; a second one appears once the first is set; each has an X; a **Side by side | Top and bottom** switch shows when any is set. Bible box and Settings change the church default; Item edit changes the item only and re-looks up on change (Task 3). Below 1500 px the Bible box controls move into the Bible chip menu as "Also show…".

- [ ] **Step 1: Write the failing tests** (Slint testing backend, element ids as named):
  - `also_picker_adds_second_then_third_slot`: pick WEB in `also-0`; `also-1` appears; pick BSB; no `also-2`.
  - `x_removes_and_compacts`: X on `also-0` moves BSB to `also-0`.
  - `item_edit_also_changes_item_not_settings`.
  - `narrow_window_folds_also_into_chip_menu` at 1400 px.
- [ ] **Step 2–4:** FAIL → implement → full test command PASS.
- [ ] **Step 5: Commit** `feat(ui): Also show pickers and layout switch`.

### Task 9: Online text stays capped, with a preflight warning

**Files:**
- Modify: `src/app.rs` (add, update, load), `src/preflight.rs`, `src/strings/preflight.rs`, `src/bible/sources.rs` (cache eviction), `src/scripture/fetch.rs`, `src/scripture/plan/cache.rs`
- Delete: `src/scripture/cache.rs` (`TextCache`)

**Interfaces:**
- Consumes: `BibleMeta.source`, `sources::Cache` (private `Cache::path`) and `Provider::cache_policy`, `scripture::fetch`.
- Produces:
  - `scripture::stored(meta: &BibleMeta) -> bool`: true for built-in, imported and downloaded Bibles.
  - On add, update and load, `verses` are cleared on the item and on each `AlsoShow` whose Bible isn't `stored`.
  - `sources::Cache::pin(&self, translation: &str, ids: &BTreeSet<VerseId>)`: pinned verses are evicted last. Opening a service pins its verses and fetches what's missing in the background.
  - `preflight::check` gains `cached: &dyn Fn(&str, &str) -> bool` (translation id, reference) and adds `strings::not_offline(abbr, reference)` = `"{abbr} isn't ready offline for {reference}. Open this service once with internet."` per uncached online Bible per item.
  - At slide build time, an uncached online Also Bible leaves its column out with note `"{abbr} needs the internet for this passage."`; an uncached online main Bible keeps FEAT-5's "Couldn't get {abbr} text — check the internet." with Try again.
  - Library §3.3.1 Remove message becomes `"Used in {n} services. Those passages won't show until it's added again."`.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test] async fn online_text_never_saved() {
    add_item_with_also(&app, "KJV", &["yv:111"]).await;
    let saved = std::fs::read_to_string(root.join("data.json")).unwrap();
    assert!(!saved.contains("For God so loved")); // NIV text from the stub reader
    assert!(saved.contains("For God so loved the world, that he gave")); // KJV, stored
}
#[test] fn preflight_flags_uncached_online_passage() {
    let out = check(&data_with_niv_item(), &monitors(), &[], &|_| true, &|_, _| false);
    assert!(out.iter().any(|l| l.ends_with("NIV isn't ready offline for John 3:16. Open this service once with internet.")));
}
#[test] fn preflight_quiet_when_cached() { /* same data, cached = |_, _| true: no such line */ }
#[test] fn uncached_online_also_is_dropped_with_note() {
    let b = build_scripture(&data_offline(), &kjv_plus_niv_item());
    let (notes, slides) = (b.notes, b.slides);
    assert_eq!(scripture_blocks(&slides[0]).len(), 1);
    assert!(notes.contains(&"NIV needs the internet for this passage.".to_string()));
}
#[test] fn pinned_verses_evicted_last() {
    // cap 3: pin JHN.3.16, put 4 others; JHN.3.16 still cached
}
```

- [ ] **Step 2–4:** FAIL → implement (move `TextCache`'s LRU and pin logic into `sources::Cache`, then delete `scripture/cache.rs`) → full test command PASS.
- [ ] **Step 5: Commit** `feat(bible): online Bible text kept only in the capped cache, with a preflight check`.

### Task 10: One row per Bible across sources

**Files:**
- Modify: `src/bible/catalog/mod.rs`

**Interfaces:**
- Consumes: `CatalogRow`, `CatalogSource::offline()`, saved keys (`sources::Secrets`).
- Produces: `catalog::merge(rows: Vec<CatalogRow>, has_key: &dyn Fn(CatalogSource) -> bool) -> Vec<CatalogRow>`: rows with equal `lang` and equal `blocklist::normalise(title_en or title)` become one. Winner: offline source first, then a source with a key, then catalog order. The winner's `also_from: Vec<CatalogSource>` (new field) lists the others, shown in the row tag as "Also on YouVersion". `App::bible_catalog` calls it after collecting all sources.

- [ ] **Step 1: Write the failing tests:** `same_bible_two_sources_one_row_offline_wins`; `different_language_same_title_kept_apart`; `keyed_online_beats_unkeyed_online`.
- [ ] **Step 2–4:** FAIL → implement → full test command PASS.
- [ ] **Step 5: Commit** `feat(bible): merge the same Bible from several libraries into one row`.

### Task 11: Door43 source

**Files:**
- Create: `src/bible/catalog/door43.rs`, `src/bible/catalog/door43/tests.rs`, fixture `tests/fixtures/door43/catalog.json` (a trimmed real response saved by hand), `tests/fixtures/door43/ulb.zip` (two small USFM books)
- Modify: `src/bible/catalog/mod.rs` (`CatalogSource::Door43`), `src/bible/catalog/install.rs`

**Interfaces:**
- Consumes: `CatalogRow::checked`, `Licence`, the eBible install path's verse writer and `BibleMeta`.
- Produces: `door43::refresh(ctx: &Ctx) -> Result<Listing, CatalogError>` from `https://api.door43.org/v3/catalog.json`; `door43::offer(entry) -> Option<CatalogRow>`; `install::door43(...)` downloading the resource's `application/zip; type=bundle content=text/usfm` format and reading its USFM.

Rules: Bible resources only (categories `bible-ot` / `bible-nt`); `checking_level == "3"` only; `rights` must map to a known `Licence` (public domain or a CC licence), else not offered; credit = `rights` plus the resource title and `checking_entity`; same politeness as eBible (one download at a time, User-Agent, daily conditional catalog fetch).

- [ ] **Step 0: Check the live shape.** Fetch the catalog once from Windows or a browser and save a trimmed copy as the fixture. If it doesn't match the Door43 docs (`languages[].resources[].formats`, `rights`, `checking.checking_level`), write down what differs in the handoff and follow the live shape.
- [ ] **Step 1: Write the failing tests:** `offers_checked_level_3_cc_by_sa`; `skips_level_2`; `skips_unknown_rights`; `blocked_title_never_offered` (a fixture entry titled "New World Translation"); `installs_usfm_zip_to_verses` (John 3:16 present with `id`).
- [ ] **Step 2–4:** FAIL → implement → full test command PASS.
- [ ] **Step 5: Commit** `feat(bible): Door43 Bibles in the library`.

### Task 12: CrossWire SWORD source

**Files:**
- Create: `src/bible/catalog/sword.rs`, `src/bible/catalog/sword/{conf.rs,ztext.rs,tests.rs}`, fixtures under `tests/fixtures/sword/` (one small public-domain module's `.conf` and data files)
- Modify: `src/bible/catalog/mod.rs` (`CatalogSource::Sword`), `src/bible/catalog/install.rs`

**Interfaces:**
- Produces: `sword::conf::parse(text: &str) -> Conf` (`name, description, lang, distribution_license, mod_drv, versification, about`); `sword::allowed(license: &str) -> Option<Licence>`; `sword::refresh` reading CrossWire's main repository listing (`mods.d.tar.gz`); `install::sword` reading `zText` modules.

Rules: `allowed` accepts "Public Domain", "GPL" and any "Creative Commons" licence; it refuses everything else, including "Copyrighted; Permission to distribute granted to CrossWire" and "Copyrighted; Free non-commercial distribution". Only `ModDrv=zText` (or `zText4`) Bibles are offered. Versification comes from the `.conf` and maps to Library §5.4's `Vsys` (unknown → not offered).

- [ ] **Step 0: Size check.** Reading `zText` needs zlib inflate. `zip` (with `deflate`) is in `Cargo.toml`; check with `cargo tree` whether `flate2` or `miniz_oxide` is already pulled in and use it directly. If any new crate is needed, stop and ask Jake before adding it, since the Nov 5 spec says SWORD ships last when it isn't small.
- [ ] **Step 1: Write the failing tests:** `crosswire_only_permission_refused`; `public_domain_and_cc_accepted`; `rawtext_driver_not_offered`; `reads_john_3_16_from_ztext_fixture`; `blocked_module_never_offered`.
- [ ] **Step 2–4:** FAIL → implement → full test command PASS.
- [ ] **Step 5: Commit** `feat(bible): CrossWire SWORD Bibles in the library`.

### Task 13: Windows check and handoff

**Files:**
- Modify: `windows-check.cmd` (build `bible-three`), `PROJECT_PLAN.md` (§8 row, §12 handoff)

- [ ] **Step 1:** Build `bible-three` on Windows with `windows-check.cmd`. Expected: build passes, `summary.txt` shows 0 failed.
- [ ] **Step 2: By-hand check with Jake** (list it in the handoff; nothing here is done by a tool): add John 3:16-18 in KJV with WEB and a Spanish Bible; check side by side and top and bottom on the room screen from the back of the room; check the stage; check the phone; pull the network and open the service (preflight shows the online Bible); add a Door43 Bible.
- [ ] **Step 3:** Record the results and anything left over in the handoff. Commit `docs: three Bibles and more sources handoff`.
