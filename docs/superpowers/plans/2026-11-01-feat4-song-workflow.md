# FEAT-4 — Song Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Coloured song sections, jump keys (`C`, `B`, `1`–`9`…) and phone section chips, safe live editing (quiet adoption, a change banner, **Update screen** / `Command::Refresh`, **Edit words**, **Save and show now**) and drag-to-reorder arrangements with a per-service order override.

**Architecture:** All decisions are made in one new pure core module, `src/sections/` (section kinds and palette, `SectionRef` tagging, occurrences, the jump rule, position mapping, the reconcile decision, arrangement editing helpers and the FEAT-4 strings). It works on any slide type through a small `SlideView` trait, so it builds and is tested **now**, while Plan 3 and Plan 4 rewrite `app.rs`, `live.rs`, `model.rs`, the UI and the server (Part A). Part B adds the data fields, the `Live` commands and state, the `App` callers, the phone and the Slint UI, written against Plan 3's file layout; each Part B task is a thin layer that calls Part A functions and applies their results.

**Tech Stack:** Rust 2024, serde/serde_json, uuid (all already in `Cargo.toml`); no new crates. Part B: Slint 1.18 (Plan 3), `i-slint-backend-testing`, the phone page `web/remote.js`.

**Spec:** `docs/superpowers/specs/2026-11-01-feat4-song-workflow-design.md` (FEAT-4). Also controlling: v1 spec `2026-10-29-rustypresenter-v1-design.md` §5.4, §6, §7 (FEAT-4 amends only the §7.2 row "Live item edited in library"); FEAT-2 §4.3/§5.2/§5.11 (`Change`, `commit_change`, `Automation::on_adopt`, phone allowlist); FEAT-1 §7 (key table, Settings tabs, layout budget, merge-by-key `service/update`); FEAT-5 §5.6 (`slides::index_for_source`, `Update::PlansReady`). Plan 3 = `docs/superpowers/plans/2026-11-01-native-operator.md`; Plan 4 = `docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md`.

## Global Constraints

- **The split.** Part A creates only new files under `src/sections/` plus the one line `pub mod sections;` in `src/lib.rs` (after `pub mod room;`, alphabetical). Part A edits **nothing else**: not `src/app.rs`, `src/live.rs`, `src/model.rs`, `src/store.rs`, `src/room.rs`, `src/slides.rs`, `src/server.rs`, `src/ui/**`, `ui/**`, `web/**`, `src/bible*`, `Cargo.toml`. Part A may *call* existing public items read-only (`model::{Slide, SlideContent, Song, Settings}`, `slides::{section_label, parse_sections, chunk_lines, song_slides}`).
- **Part B is blocked** until the `native-foundation` branch has Plan 3 **and** Plan 4 merged (spec §10: Plan 3 Tasks 6, 7, 9, 10, 17, 18 at minimum). Part B tasks that touch `Live` also need FEAT-2's `Change`/`commit_change` refactor (FEAT-2 §8 step 4); Task B8 says what to do if it has not landed.
- **Part A test command** (PowerShell, from the worktree root):
  `& "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd" test --release --lib <module path>` — written below as `feat-cargo test --release --lib <module path>`.
- **Serde:** every new persisted field has a serde default and old `data.json` loads unchanged; no `Data.format` bump (spec §4.1, D17). `SectionRef` is built, never stored in `data.json`.
- **Section kinds and colours are fixed** (D2, D4); colours never reach the room output or the compositor (spec §6). The colour is never shown alone: the short name is always printed next to it.
- **Palette (spec §6.1, adjusted per its "adjusted in lightness before release" rule).** The spec's values fail its own test (`palette_is_distinct_and_not_live_red`, CIEDE2000): Verse/Intro ΔE 9.8, Intro/Ending 13.0, Chorus/live 24.9, Tag/live 23.2. Lightness-only (HSL L) adjustments that pass, keeping hue and saturation: **Chorus `#d148a7`, Tag `#f4aa71`, Intro `#a5c1d9`, Ending `#838da0`**. Unchanged: Verse `#4c8dff`, Pre-chorus `#2bb3a8`, Bridge `#8b6cf0`, Interlude `#b5a254`, Other `#5d6576`. Result: minimum pairwise ΔE 15.8, minimum ΔE to `#ff4f55` 25.6, minimum contrast against `#15181e` 4.4:1. Task B17 updates the spec's §6.1 table to these values. `SectionKind::color` is the only place the hex values are written in Rust; `ui/theme.slint` copies them and a test compares the two.
- **Strings:** every user-visible FEAT-4 text is a `pub const` (or a `fn` for formatted ones) in `src/sections/strings.rs` in Part A. Task B1 moves the file to `src/strings/song.rs` (the shared `src/strings/<area>.rs` convention, spec §7) once `src/strings/mod.rs` exists on the merged branch, leaving `pub use crate::strings::song as strings;` in `src/sections/mod.rs` so no caller changes. Wording is copied verbatim from the spec.
- **Stale text** for the phone and chip clicks is "The screen changed — try again."; the operator key path hides stale outcomes (Plan 3 `outcome_toast`).
- **Lock order** is always data → live (spec §5.4), including in `App::room_event`.
- **Files** stay under about 400 lines each. Part A tests live in `#[cfg(test)] mod tests` inside each file.

### Product decisions taken in this plan (spec leaves them open)

1. **`V` key:** first Verse occurrence that starts after the current occurrence; with none ahead, rules 1 and 3 of the jump rule apply (restart the current verse, else the last verse before).
2. **`Occurrence` and `OccurrenceView` gain `verse_nth: Option<u8>`** (the spec's `Occurrence` lacks it, but key hints for unnumbered verses need it).
3. **Numbers in short names:** every kind except Other appends its number (`V2`, `C2`, `PC2`); Other is always `•` and `from_label` gives Other no number (`Part 3` → `(Other, None)`).
4. **Toast after a jump** follows spec §3.2/§7 literally: "Jumped to <Name> (<key>)", where Name is the kind's display name (`SectionKind::name()`) plus the digit's number for digit keys: "Jumped to Chorus (C)", "Jumped to Verse 2 (2)", "Jumped to Verse (V)". It is built from the key the operator pressed (known before sending), not from the landed slide, so the operator never reads `LiveView` after the command (no race with broadcasts); it is shown only on an `Ok` outcome. The key is printed upper-case.
5. **`heading_offset` / `caret_for_occurrence`**: `heading_offset` returns a UTF-8 **byte** offset (what Slint's `set-selection-offsets` takes) of the first lyric line after the `nth` heading whose label matches (case-insensitive, trimmed); `None` for unlabelled songs ("Part N") and missing headings (caret then goes to 0). Edit words maps the live occurrence to its written heading with `caret_for_occurrence` (Task A7), which takes the section index from `resolve_order`, so duplicate plain `Verse` headings and repeated arranged choruses land on the right heading.
6. **Moving a chip** follows Plan 3's service-list rule: remove at `from`, insert at `to` clamped to the new length (`[a,b,c]`, 0→2 gives `[b,c,a]`; the end zone `to = len` gives the same as `to = len-1`).
7. **Slide labels stay per blank-line group as today** ("Verse 1 (1/2)", spec §5.1 "labels do not change"), while `SectionRef.page/pages` count across all of the section's groups (spec §5.1). The two differ for a section with a blank-line split; the stage label (built from `SectionRef`) is the consistent one. Pinned by `section_refs_count_pages_across_groups` (Task A2). This contradicts the spec §5.1 parenthetical "(so "Bridge (2/3)" labels and `page` agree)"; Task B17 corrects that sentence.

Decisions 1, 4 and 7 are listed for Jake in Task B17's spec update (spec §11).

## Review Focus

1. **Duplicate headings in the lyrics** (a hymn with four plain `Verse` headings, or `Chorus` written twice). Expected: every written section plays when the arrangement is empty, each `Verse` gets its own `verse_nth` (1, 2, 3, 4) so digit `3` reaches the third verse, and ordinals count per label. Tests `duplicate_plain_verse_headings_get_distinct_verse_nth` (Task A2) and `caret_for_occurrence_with_duplicate_plain_verses` (Task A7: Edit words lands on the right written `Verse`).
2. **Arrangement entries typed with different case or spaces** (`" chorus "`, `VERSE 1`). Expected: they match the section exactly as today's `song_order` does (trimmed, case-insensitive), and occurrence/ordinal counting treats them as one label. Test `order_entries_match_trimmed_case_insensitive` in Task A2.
3. **A song with ten or more verses.** Expected: `Verse 10` gets a chip and short name `V10` but no key hint, and digit `1` still means Verse 1, never Verse 10. Test `verse_ten_has_no_key_hint` in Task A3.
4. **CRLF lyrics and non-ASCII text before the live section** ("Él es…", curly quotes, emoji). Expected: Edit words puts the caret exactly at the first lyric line of the section, on a char boundary. Test `heading_offset_handles_crlf_and_multibyte` in Task A7.
5. **A section split by a blank line** ("Verse 1" with two groups). Expected: thumbnails keep today's labels, `SectionRef.page/pages` count across groups (1/3, 2/3, 3/3), and quiet adoption after an edit inside the second group lands on the same physical page. Tests `section_refs_count_pages_across_groups` (Task A2) and `map_exact_across_blank_line_groups` (Task A5).

---

# PART A — Core, buildable now

All Part A files are new. Module root:

```
src/sections/mod.rs          module docs, `pub use` of the public items below
src/sections/strings.rs      FEAT-4 user-visible text (moved to src/strings/song.rs in B1)
src/sections/kind.rs         SectionKind, palette, color_for_label (+ CIEDE2000 test helper)
src/sections/refs.rs         SectionRef, SlideView, order resolution, SectionRef tagging, test kit
src/sections/occurrences.rs  Occurrence, OccurrenceView, key hints
src/sections/jump.rs         JumpTarget, JumpError, jump_target, key → jump, toast text
src/sections/map.rs          same_on_screen, Mapped, map_position
src/sections/reconcile.rs    ChangeReason, LiveChange, Decision, decide, adopt_map, refresh_slide
src/sections/arrangement.rs  chip strip editing helpers, heading_offset
```

### Task A1: Module root, strings, `SectionKind` and the palette

**Files:**
- Create: `src/sections/mod.rs`, `src/sections/strings.rs`, `src/sections/kind.rs`
- Modify: `src/lib.rs` (one line: `pub mod sections;`)

**Interfaces:**
- Produces (`kind.rs`, re-exported from `sections`):
  - `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)] #[serde(rename_all = "snake_case")] pub enum SectionKind { Verse, Chorus, PreChorus, Bridge, Tag, Intro, Ending, Interlude, Other }`
  - `impl SectionKind { pub fn from_label(label: &str) -> (SectionKind, Option<u8>); pub fn short(self, number: Option<u8>) -> String; pub fn color(self) -> &'static str; pub fn key(self) -> Option<char>; pub fn name(self) -> &'static str; pub const ALL: [SectionKind; 9]; }`
  - `pub fn color_for_label(label: &str) -> Option<&'static str>` (None for Other).
- `from_label` rules (spec §4.1): lower-case, trim, strip `[]` and a trailing `:`; first word verse→Verse; chorus, refrain→Chorus; pre-chorus, prechorus, and the two words "pre chorus"→PreChorus; bridge→Bridge; tag→Tag; intro→Intro; ending, end, outro, coda→Ending; interlude, instrumental, turnaround, vamp→Interlude; anything else (post-chorus, misc, "Part N", custom bracketed labels) → Other. Number = the first suffix word that is all ASCII digits, 1–2 digits, value 1–99; Other never has a number.
- Short: Verse `V`, Chorus `C`, PreChorus `PC`, Bridge `B`, Tag `T`, Intro `I`, Ending `E`, Interlude `Int`, each followed by the number if any; Other `•`.
- Keys: Verse `'v'`, Chorus `'c'`, PreChorus `'p'`, Bridge `'b'`, Tag `'t'`, Intro `'i'`, Ending `'e'`, Interlude `'l'`, Other `None`.
- Names (from `strings.rs`): "Verse", "Chorus", "Pre-chorus", "Bridge", "Tag", "Intro", "Ending", "Interlude", "Other".
- Colours: the adjusted palette in Global Constraints, lower-case `#rrggbb`.
- `strings.rs` holds every FEAT-4 text from spec §3, §5, §7, §8, as `pub const` or `pub fn`: the nine kind names; `NOTHING_LIVE = "Nothing is live yet."`; `KEYS_SONGS_ONLY = "Section keys work on songs."`; `fn no_section(name: &str, number: Option<u8>) -> String` ("This song has no Chorus." / "This song has no Verse 4."); `SONG_CHANGED = "The song changed — try again."`; `LIVE_SONG_CHANGED = "The live song changed — try again."`; `SCREEN_CHANGED = "The screen changed — try again."`; `fn jumped(label: &str, key: char) -> String` ("Jumped to Chorus (C)"); `NO_LONGER_LIVE = "That song is no longer live."`; `NO_SLIDES_NOW = "That song has no slides now. The screen keeps the old words."`; `SAVED_TEXT_CLEARED = "Saved. The new words show when you bring the text back."`; `LIVE_EDIT_BANNER = "This song is on screen. Nothing changes on screen until you choose Save and show now or Update screen."`; the four change banners `BANNER_WORDS`, `BANNER_LOOK`, `BANNER_LOST`, `BANNER_NO_SLIDES` (spec §8, verbatim); `UPDATE_SCREEN = "Update screen"`; `EDIT_WORDS = "Edit words"`; `SAVE_AND_SHOW = "Save and show now"`; `NOT_IN_LYRICS = "Not in the lyrics — it is skipped"`; `WRITTEN_ORDER_HINT = "Written order — drag to change"`; `USE_WRITTEN_ORDER = "Use written order"`; `CUSTOM_ORDER = "Custom order"`; `CUSTOM_ORDER_HEADER = "Custom order for this service"`; `ORDER_FOR_SERVICE = "Order for this service"`; `USE_USUAL_ORDER = "Use the song's usual order"`; `MAKE_USUAL_ORDER = "Make this the song's usual order"`; `SAVED_AS_USUAL = "Saved as the usual order."`; `SETTING_SECTION_KEYS = "Section keys (C, B, 1–9…)"`; `SETTING_SECTION_KEYS_HINT = "Turn off if letters typed by mistake move the screen."`; `CHIP_SHOW_HINT = "Click to show this part"`; `fn chip_key_hint(label: &str, key: char) -> String` ("Verse 2 — press 2"); `fn live_section_line(label: &str, page: u16, pages: u16) -> String` ("Chorus · slide 2 of 3"); Help rows `HELP_DIGITS = "1–9 — Verse 1–9 of the live song"`, `HELP_LETTERS = "C P B T I E L V — Chorus, Pre-chorus, Bridge, Tag, Intro, Ending, Interlude, next Verse"`.

- [ ] **Step 1: Write the failing tests** in `src/sections/kind.rs`:

```rust
#[test]
fn from_label_kinds_and_numbers() {
    use SectionKind::*;
    let t = |l| SectionKind::from_label(l);
    assert_eq!(t("Verse 1"), (Verse, Some(1)));
    assert_eq!(t("Chorus 2"), (Chorus, Some(2)));
    assert_eq!(t("Pre-Chorus"), (PreChorus, None));
    assert_eq!(t("pre chorus"), (PreChorus, None));
    assert_eq!(t("Refrain"), (Chorus, None));
    for l in ["Outro", "End", "Coda", "Ending"] { assert_eq!(t(l), (Ending, None)); }
    assert_eq!(t("Vamp"), (Interlude, None));
    assert_eq!(t("Part 3"), (Other, None));
    assert_eq!(t("Spontaneous"), (Other, None));
    assert_eq!(t("Post-Chorus"), (Other, None));
    assert_eq!(t("Verse 2a"), (Verse, None));
    assert_eq!(t("Verse 0"), (Verse, None));
    assert_eq!(t("Verse 100"), (Verse, None));
    assert_eq!(t("[Bridge]:"), (Bridge, None));
}

#[test]
fn short_names() {
    use SectionKind::*;
    assert_eq!(Verse.short(Some(2)), "V2");
    assert_eq!(Chorus.short(None), "C");
    assert_eq!(PreChorus.short(None), "PC");
    assert_eq!(Bridge.short(None), "B");
    assert_eq!(Interlude.short(None), "Int");
    assert_eq!(Other.short(Some(3)), "•");
}

#[test]
fn keys_and_names() {
    use SectionKind::*;
    let keys: Vec<_> = SectionKind::ALL.iter().map(|k| k.key()).collect();
    assert_eq!(keys, [Some('v'), Some('c'), Some('p'), Some('b'), Some('t'), Some('i'), Some('e'), Some('l'), None]);
    assert_eq!(PreChorus.name(), "Pre-chorus");
}

#[test]
fn color_for_label_matches_kind_color() {
    assert_eq!(color_for_label("Chorus 2"), Some(SectionKind::Chorus.color()));
    assert_eq!(color_for_label("Part 1"), None);
}

#[test]
fn serde_is_snake_case() {
    assert_eq!(serde_json::to_string(&SectionKind::PreChorus).unwrap(), "\"pre_chorus\"");
}

#[test]
fn ciede2000_reference_pairs() {
    // Sharma, Wu, Dalal (2005) test data, pairs 1 and 17.
    assert!((de2000_lab([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485]) - 2.0425).abs() < 1e-3);
    assert!((de2000_lab([50.0, 2.5, 0.0], [73.0, 25.0, -18.0]) - 27.1492).abs() < 1e-3);
    assert!((delta_e2000("#000000", "#ffffff") - 100.0).abs() < 0.05);
}

#[test]
fn palette_is_distinct_and_not_live_red() {
    let named: Vec<_> = SectionKind::ALL.iter().filter(|k| **k != SectionKind::Other).collect();
    for (i, a) in named.iter().enumerate() {
        for b in &named[i + 1..] {
            let d = delta_e2000(a.color(), b.color());
            assert!(d >= 15.0, "{a:?}/{b:?} ΔE {d:.1}");
        }
        let d = delta_e2000(a.color(), "#ff4f55");
        assert!(d >= 25.0, "{a:?} vs live ΔE {d:.1}");
    }
}

#[test]
fn band_colours_contrast_with_panel() {
    for k in SectionKind::ALL.iter().filter(|k| **k != SectionKind::Other) {
        assert!(contrast(k.color(), "#15181e") >= 3.0, "{k:?}");
    }
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::kind`. Expected: FAIL to compile (`sections` module missing).
- [ ] **Step 3: Implement** `mod.rs` (`pub mod strings; mod kind; …; pub use kind::*;`), `strings.rs`, `kind.rs`, and the `lib.rs` line. Test-only helpers in `kind.rs` `#[cfg(test)]`: `fn de2000_lab(a: [f64;3], b: [f64;3]) -> f64` (standard CIEDE2000, kL = kC = kH = 1), `fn delta_e2000(a: &str, b: &str) -> f64` (hex → sRGB, linearise with threshold 0.04045, XYZ with the D65 matrix `[0.4124, 0.3576, 0.1805; 0.2126, 0.7152, 0.0722; 0.0193, 0.1192, 0.9505]`, white `0.95047, 1.0, 1.08883`, Lab with ε 0.008856 and 7.787·t + 16/116), and `fn contrast(a, b) -> f64` (WCAG 2 relative luminance).
- [ ] **Step 4: Run** `feat-cargo test --release --lib sections::kind`. Expected: PASS (8 tests). If `palette_is_distinct_and_not_live_red` fails with the colours above, the ΔE helper is wrong: check it against the reference pairs first, never loosen the thresholds.
- [ ] **Step 5: Commit** `feat(sections): section kinds, fixed palette and FEAT-4 strings`.

### Task A2: `SectionRef`, `SlideView`, order resolution and tagging

**Files:**
- Create: `src/sections/refs.rs`

**Interfaces:**
- Consumes: `SectionKind::from_label` (A1); `crate::slides::{parse_sections, chunk_lines, song_slides}`, `crate::model::{Slide, SlideContent, Song, Settings}` (read only).
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)] pub struct SectionRef { pub label: String, pub kind: SectionKind, pub number: Option<u8>, pub verse_nth: Option<u8>, pub occurrence: u16, pub ordinal: u16, pub page: u16, pub pages: u16 }` (fields exactly as spec §4.1).
  - `pub trait SlideView { fn label(&self) -> &str; fn content(&self) -> &SlideContent; fn notes(&self) -> &str; fn section(&self) -> Option<&SectionRef>; fn eq_except_notes(&self, other: &Self) -> bool where Self: Sized { self.label() == other.label() && self.content() == other.content() && self.section() == other.section() } }`
  - `impl SlideView for crate::model::Slide` with `section()` returning `None` for now (Task B1 changes it to `self.section.as_ref()` and overrides `eq_except_notes` to include FEAT-5 `source`).
  - `pub fn resolve_order(order: &[String], labels: &[String]) -> Vec<usize>`: today's `slides::song_order` rule (each entry trimmed, matched case-insensitively against the first section with that label; unknown entries skipped; if none match, `0..labels.len()`). Task B2 makes `slides::song_order` delegate to it.
  - `pub fn pick_order<'a>(item_override: Option<&'a [String]>, song: &'a [String]) -> &'a [String]` (`item_override.unwrap_or(song)`; `Some(&[])` stays empty, which means written order).
  - `pub fn song_section_refs(lyrics: &str, order: &[String], lines_per_slide: usize) -> Vec<SectionRef>`: one `SectionRef` per page in played order, the same count and order as `slides::song_slides` produces. `occurrence` counts played entries; `ordinal` counts earlier occurrences with the same lower-cased label; `verse_nth` numbers the distinct Verse **sections** (by section index, not label) in written order, 1-based, and is set on Verse refs only; `page`/`pages` count across all blank-line groups of the section.
- Test kit (`#[cfg(test)] pub(crate) mod kit` in `refs.rs`): `pub struct TSlide { pub label: String, pub content: SlideContent, pub notes: String, pub section: Option<SectionRef> }` (`Clone, Debug, PartialEq`, `impl SlideView`), `pub fn song(lyrics: &str, order: &[&str], lps: u8) -> Vec<TSlide>` (zips `slides::song_slides(&Song { lyrics, arrangement: order, ..Default::default() }, &Settings { credits: "none".into(), lines_per_slide: lps, ..Default::default() })` with `song_section_refs`, asserting equal lengths), `pub fn text(s: &TSlide) -> &str`, `pub fn plain(texts: &[&str]) -> Vec<TSlide>` (non-song slides, `section: None`). Later tasks' tests use this kit.

- [ ] **Step 1: Write the failing tests** in `src/sections/refs.rs`:

```rust
const SONG: &str = "Verse 1\na\nb\nChorus\nx\ny\nz\nVerse 2\nc\nd";

#[test]
fn song_slides_tag_section_ref() {
    // V1 C V2 C, two lines per slide: chorus has 2 pages.
    let s = kit::song(SONG, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 2);
    let r: Vec<_> = s.iter().map(|s| s.section.clone().unwrap()).collect();
    assert_eq!(r.len(), 6);
    assert_eq!((r[1].label.as_str(), r[1].occurrence, r[1].ordinal, r[1].page, r[1].pages), ("Chorus", 1, 0, 0, 2));
    assert_eq!((r[2].page, r[2].pages), (1, 2));
    assert_eq!((r[3].label.as_str(), r[3].occurrence, r[3].number), ("Verse 2", 2, Some(2)));
    assert_eq!((r[4].occurrence, r[4].ordinal), (3, 1));
}

#[test]
fn repeated_chorus_gets_distinct_ordinals() { /* C V1 C C → chorus ordinals 0, 1, 2 */ }

#[test]
fn unnumbered_verses_get_verse_nth() {
    let s = kit::song("Verse\na\nChorus\nx\nVerse\nb", &[], 4);
    let nth: Vec<_> = s.iter().map(|s| s.section.as_ref().unwrap().verse_nth).collect();
    assert_eq!(nth, [Some(1), None, Some(2)]);
}

#[test]
fn duplicate_plain_verse_headings_get_distinct_verse_nth() {
    let s = kit::song("Verse\na\nVerse\nb\nVerse\nc\nVerse\nd", &[], 4);
    let r: Vec<_> = s.iter().map(|s| s.section.clone().unwrap()).collect();
    assert_eq!(r.iter().map(|r| r.verse_nth).collect::<Vec<_>>(), [Some(1), Some(2), Some(3), Some(4)]);
    assert_eq!(r.iter().map(|r| r.ordinal).collect::<Vec<_>>(), [0, 1, 2, 3]);
}

#[test]
fn section_refs_count_pages_across_groups() {
    // Verse 1 has two blank-line groups: [a b c] and [d]; 2 lines per slide → pages a/b, c, d.
    let s = kit::song("Verse 1\na\nb\nc\n\nd", &[], 2);
    let r: Vec<_> = s.iter().map(|s| { let r = s.section.as_ref().unwrap(); (r.page, r.pages) }).collect();
    assert_eq!(r, [(0, 3), (1, 3), (2, 3)]);
    assert_eq!(s.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Verse 1 (1/2)", "Verse 1 (2/2)", "Verse 1"]); // labels unchanged
}

#[test]
fn order_entries_match_trimmed_case_insensitive() {
    let labels = vec!["Verse 1".to_string(), "Chorus".to_string()];
    assert_eq!(resolve_order(&[" chorus ".into(), "VERSE 1".into(), "Nope".into()], &labels), [1, 0]);
    assert_eq!(resolve_order(&["Nope".into()], &labels), [0, 1]);
}

#[test]
fn item_order_prefers_item_override() {
    let song = vec!["Chorus".to_string()];
    let item = vec!["Verse 1".to_string()];
    assert_eq!(pick_order(Some(item.as_slice()), &song), &item[..]);
    assert_eq!(pick_order(None, &song), &song[..]);
}

#[test]
fn empty_override_list_means_written_order() {
    // Spec §9.1: Some(vec![]) on the item = written order, even when the song has its own order.
    let song_order = vec!["Chorus".to_string()];
    let picked = pick_order(Some(&[]), &song_order);
    assert!(picked.is_empty());
    let labels: Vec<String> = ["Verse 1", "Chorus", "Verse 2"].map(String::from).to_vec();
    assert_eq!(resolve_order(picked, &labels), (0..labels.len()).collect::<Vec<_>>());
    let refs = song_section_refs(SONG, picked, 4);
    assert_eq!(refs.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["Verse 1", "Chorus", "Verse 2"]);
}

#[test]
fn section_ref_json_round_trip() {
    let r = kit::song(SONG, &[], 4)[1].section.clone().unwrap();
    let j = serde_json::to_value(&r).unwrap();
    assert_eq!(j["kind"], "chorus");
    assert_eq!(serde_json::from_value::<SectionRef>(j).unwrap(), r);
}

#[test]
fn model_slide_has_no_section_until_b1() {
    let s = crate::model::Slide { label: "x".into(), content: SlideContent::Text { text: "a".into(), footer: None, reference: None }, notes: String::new() };
    assert!(SlideView::section(&s).is_none());
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::refs`. Expected: FAIL (items not defined).
- [ ] **Step 3: Implement** in `refs.rs`. `song_section_refs` re-runs `parse_sections` + `resolve_order` + `chunk_lines` exactly as `song_slides` does (pages per group = `chunk_lines(group, lps).len()`), so the counts always agree; the kit's length assertion is the guard.
- [ ] **Step 4: Run** `feat-cargo test --release --lib sections::refs`. Expected: PASS (10 tests).
- [ ] **Step 5: Commit** `feat(sections): SectionRef tagging, order resolution and SlideView`.

### Task A3: Occurrences, the live view list and key hints

**Files:**
- Create: `src/sections/occurrences.rs`

**Interfaces:**
- Consumes: `SlideView`, `SectionRef`, `SectionKind` (A1, A2).
- Produces:
  - `#[derive(Serialize, Clone, Debug, PartialEq)] pub struct Occurrence { pub occurrence: u16, pub label: String, pub kind: SectionKind, pub number: Option<u8>, pub verse_nth: Option<u8>, pub first_slide: usize, pub slides: usize }`
  - `pub fn occurrences<S: SlideView>(slides: &[S]) -> Vec<Occurrence>`: one per distinct `SectionRef.occurrence`, in slide order; slides with `section() == None` are skipped; empty for non-song items.
  - `pub fn numbered_verses(occ: &[Occurrence]) -> bool`: true when any Verse occurrence has a number.
  - `pub fn key_hint(o: &Occurrence, numbered: bool) -> Option<char>`: Verse → its number (or `verse_nth` when `!numbered`) as a digit if 1–9, else `None`; other kinds → `kind.key()` upper-cased.
  - `#[derive(Serialize, Clone, Debug, PartialEq)] pub struct OccurrenceView { pub occurrence: u16, pub label: String, pub short: String, pub kind: SectionKind, pub color: &'static str, pub first_slide: usize, pub slides: usize, pub live: bool, pub key: Option<char> }` (spec §4.2 plus `key`, so the operator and phone never recompute hints).
  - `pub fn occurrence_views<S: SlideView>(slides: &[S], live_slide: Option<usize>) -> Vec<OccurrenceView>`: `live` is true for the occurrence containing `live_slide`.
  - `pub fn live_section_line<S: SlideView>(slides: &[S], live_slide: usize) -> Option<(String, &'static str)>`: ("Chorus · slide 2 of 3", colour) from the live slide's `SectionRef` (label, `page + 1`, `pages`); `None` for non-song slides.

- [ ] **Step 1: Write the failing tests:** `occurrences_follow_played_order` (`V1 C V2 C` → shorts `V1 C V2 C`, `first_slide` 0,1,3,4 with a two-page chorus, `slides` 1,2,1,2), `non_song_has_no_occurrences` (`kit::plain`), `live_flag_marks_only_the_live_occurrence` (live slide 2 → occurrence 1 live, others not), `key_hints_use_numbers_or_verse_nth` (numbered song: `V2` → `'2'`, `C` → `'C'`; unnumbered hymn: second Verse → `'2'`), `verse_ten_has_no_key_hint` (a song with `Verse 10` → `short == "V10"`, `key == None`; `Verse 1` → `'1'`), `other_has_no_key_hint` (`Part 1`), `live_section_line_text` (second page of a three-page chorus → `"Chorus · slide 2 of 3"` and the chorus colour).
- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::occurrences`. Expected: FAIL.
- [ ] **Step 3: Implement** in `occurrences.rs`.
- [ ] **Step 4: Run** the same command. Expected: PASS (7 tests).
- [ ] **Step 5: Commit** `feat(sections): occurrences, live section line and key hints`.

### Task A4: The jump rule, key mapping and jump text

**Files:**
- Create: `src/sections/jump.rs`

**Interfaces:**
- Consumes: `occurrences`, `numbered_verses` (A3), `SectionKind::name` and `strings` (A1).
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct JumpTarget { pub kind: SectionKind, pub number: Option<u8>, pub occurrence: Option<u16> }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum JumpError { NotASong, NoSuchSection(SectionKind, Option<u8>), NoSuchOccurrence, KindMismatch }` with `impl JumpError { pub fn message(&self) -> String }`: NotASong → `KEYS_SONGS_ONLY`; NoSuchSection → `no_section(kind.name(), number)`; NoSuchOccurrence and KindMismatch → `SONG_CHANGED`.
  - `pub fn jump_target<S: SlideView>(slides: &[S], current: usize, t: &JumpTarget) -> Result<usize, JumpError>` (spec §5.2 steps 3–4, §3.2):
    1. No slide has a section → `NotASong`.
    2. `occurrence: Some(o)` → first slide of occurrence `o`; missing → `NoSuchOccurrence`; its kind ≠ `t.kind` → `KindMismatch`.
    3. Else the candidates are occurrences of `t.kind` that match `t.number` (`None` = any number); for Verse with `Some(n)`, match `number == Some(n)`, or `verse_nth == Some(n)` when `!numbered_verses`.
    4. Rule 1: the current slide's occurrence is a candidate → its `first_slide`. Rule 2: the first candidate whose `first_slide > current`. Rule 3: the last candidate whose `first_slide + slides <= current`. None → `NoSuchSection(kind, number)`.
    5. **`V` (Verse, `number: None`)** tries rule 2 first, then rules 1 and 3 (product decision 1).
  - `pub fn jump_for_key(key: &str, ctrl_or_cmd: bool, alt: bool) -> Option<(SectionKind, Option<u8>)>`: a single character with neither modifier; `'1'..='9'` → `(Verse, Some(d))`; a letter whose lower case equals some `SectionKind::key()` → `(kind, None)`; `'0'`, other letters, other text → `None`.
  - `pub fn jump_toast(kind: SectionKind, number: Option<u8>, key: char) -> String` = `strings::jumped(&name, key.to_ascii_uppercase())` where `name` is `kind.name()`, followed by `" {n}"` when `number` is `Some(n)` (product decision 4, spec §3.2/§7 "Jumped to <Name> (<key>)"). The caller passes the `(kind, number)` it got from `jump_for_key`, so no landed slide is needed.

- [ ] **Step 1: Write the failing tests** (all on `kit::song`; `JS` = `"Verse 1\na\nVerse 2\nb\nChorus\nx\ny\nBridge\nz"` with `lps = 1`):

```rust
fn t(kind: SectionKind, number: Option<u8>) -> JumpTarget { JumpTarget { kind, number, occurrence: None } }

#[test]
fn jump_restarts_current_occurrence() {
    let s = kit::song(JS, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 1); // V1 C(1,2) V2 C(4,5)
    assert_eq!(jump_target(&s, 2, &t(SectionKind::Chorus, None)), Ok(1));
}
#[test]
fn jump_goes_to_next_occurrence_ahead() { /* same song, current 3 (V2) → Ok(4) */ }
#[test]
fn jump_falls_back_to_last_before() { /* current 5 is last chorus' last slide → rule 1 → Ok(4); with order V1 C V2 and current 3 → Ok(1) */ }
#[test]
fn jump_digit_selects_verse_number() { /* t(Verse, Some(2)) from 0 → Ok(3) */ }
#[test]
fn digit_falls_back_to_nth_unnumbered_verse() {
    let s = kit::song("Verse\na\nChorus\nx\nVerse\nb", &[], 1);
    assert_eq!(jump_target(&s, 0, &t(SectionKind::Verse, Some(2))), Ok(2));
}
#[test]
fn jump_v_goes_to_next_verse_any_number() {
    let s = kit::song(JS, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 1);
    assert_eq!(jump_target(&s, 0, &t(SectionKind::Verse, None)), Ok(3)); // inside V1 → next verse, not restart
    assert_eq!(jump_target(&s, 4, &t(SectionKind::Verse, None)), Ok(3)); // none ahead → last before
}
#[test]
fn jump_explicit_occurrence() {
    let s = kit::song(JS, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 1);
    assert_eq!(jump_target(&s, 0, &JumpTarget { kind: SectionKind::Chorus, number: None, occurrence: Some(3) }), Ok(4));
    assert_eq!(jump_target(&s, 0, &JumpTarget { kind: SectionKind::Chorus, number: None, occurrence: Some(9) }), Err(JumpError::NoSuchOccurrence));
}
#[test]
fn jump_occurrence_kind_mismatch_errors() { /* occurrence Some(0) with kind Chorus → Err(KindMismatch); message "The song changed — try again." */ }
#[test]
fn jump_missing_kind_errors() {
    let s = kit::song(JS, &["Verse 1", "Chorus"], 1);
    let e = jump_target(&s, 0, &t(SectionKind::Tag, None)).unwrap_err();
    assert_eq!(e.message(), "This song has no Tag.");
    assert_eq!(jump_target(&s, 0, &t(SectionKind::Verse, Some(4))).unwrap_err().message(), "This song has no Verse 4.");
}
#[test]
fn jump_on_non_song_is_not_a_song() { assert_eq!(jump_target(&kit::plain(&["a"]), 0, &t(SectionKind::Chorus, None)), Err(JumpError::NotASong)); }
#[test]
fn letters_and_digits_map_to_jumps() {
    assert_eq!(jump_for_key("c", false, false), Some((SectionKind::Chorus, None)));
    assert_eq!(jump_for_key("C", false, false), Some((SectionKind::Chorus, None)));
    assert_eq!(jump_for_key("l", false, false), Some((SectionKind::Interlude, None)));
    assert_eq!(jump_for_key("v", false, false), Some((SectionKind::Verse, None)));
    assert_eq!(jump_for_key("7", false, false), Some((SectionKind::Verse, Some(7))));
}
#[test]
fn ctrl_or_alt_letter_is_not_a_jump() { assert_eq!(jump_for_key("c", true, false), None); assert_eq!(jump_for_key("2", false, true), None); }
#[test]
fn zero_and_unmapped_letters_do_nothing() { for k in ["0", "x", "n", "cc", ""] { assert_eq!(jump_for_key(k, false, false), None, "{k}"); } }
#[test]
fn toast_names_kind_and_key() {
    assert_eq!(jump_toast(SectionKind::Verse, Some(2), '2'), "Jumped to Verse 2 (2)");
    assert_eq!(jump_toast(SectionKind::Chorus, None, 'c'), "Jumped to Chorus (C)");
    assert_eq!(jump_toast(SectionKind::PreChorus, None, 'p'), "Jumped to Pre-chorus (P)");
    assert_eq!(jump_toast(SectionKind::Verse, None, 'v'), "Jumped to Verse (V)");
}
```

- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::jump`. Expected: FAIL.
- [ ] **Step 3: Implement** in `jump.rs`.
- [ ] **Step 4: Run** the same command. Expected: PASS (14 tests).
- [ ] **Step 5: Commit** `feat(sections): jump rule, key mapping and jump messages`.

### Task A5: `same_on_screen` and `map_position`

**Files:**
- Create: `src/sections/map.rs`

**Interfaces:**
- Consumes: `SlideView`, `occurrences` (A2, A3).
- Produces:
  - `pub fn same_on_screen<S: SlideView>(a: &S, b: &S) -> bool` = `a.content() == b.content()` (text, footer, reference, asset; labels, notes and section ignored).
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Mapped { Exact(usize), Moved(usize), Lost }`
  - `pub fn map_position<S: SlideView>(old: &[S], old_index: usize, new: &[S]) -> Mapped` (spec §5.4):
    1. `old[old_index].section()` is `None`: `Exact(old_index)` if `new.len() > old_index` and `same_on_screen`; else `Lost`.
    2. Lower-cased occurrence label lists `a` (old) and `b` (new). Pair them twice with an LCS table: **forward** walk from `(0,0)` using the suffix table `dp[i][j]` (match when labels are equal; else advance `i` if `dp[i+1][j] >= dp[i][j+1]`, else `j`), and **backward** walk from `(len_a, len_b)` using the prefix table (match when equal; else retreat `i` if `pre[i-1][j] >= pre[i][j-1]`, else `j`). If both pair the live occurrence `p` with the same `q`: page `k` of the live slide; `k < pages(q)` → `Exact(first(q)+k)`, else `Moved(first(q)+pages(q)-1)`.
    3. Otherwise, the new occurrence with the same lower-cased label and the same `ordinal` → page clamped → `Moved`.
    4. Otherwise, the new occurrence with the same `occurrence` index whose `kind` and `number` equal the old one's → page clamped → `Moved`.
    5. Otherwise `Lost`.

- [ ] **Step 1: Write the failing tests** (`kit::song`, `lps = 1`, lyrics with one line per section unless stated):
  - `same_on_screen_ignores_label`, `same_on_screen_ignores_notes`, `same_on_screen_ignores_occurrence_shift` (same text, different `SectionRef.occurrence` → true), `same_on_screen_sees_footer_change` (footer `Some("x")` vs `None` → false).
  - `map_exact_same_label_ordinal_page` (identical songs, every index → `Exact(i)`).
  - `map_moved_when_pages_shrink` (chorus 3 pages → 2, live on page 3 → `Moved(last page)`).
  - `map_keeps_physical_occurrence_when_earlier_repeat_removed`: order `C V1 C V2 C`, live on the second C (index 2) → new order `V1 C V2 C` → `Exact(1)`.
  - `map_keeps_physical_occurrence_when_chorus_inserted_before`: `V1 C V2`, live on V2 (index 2) → `C V1 C V2` → `Exact(3)`.
  - `map_ambiguous_alignment_falls_back_to_ordinal`: `C C`, live on index 0 → new `C` (forward pairs 0→0, backward pairs 1→0, so they disagree for 0) → `Moved(0)`.
  - `map_moved_when_label_renamed`: `Chorus` heading renamed to `Refrain` (same kind, no number), live on it → `Moved(same index)`.
  - `map_lost_when_section_deleted`: Bridge removed from lyrics and order, live on Bridge → `Lost`.
  - `map_non_song_by_index` (`kit::plain(["a","b"])` → `["a","b","c"]`, index 1 → `Exact(1)`; `["a","B"]` → `Lost`).
  - `map_exact_across_blank_line_groups` (Review Focus 5): Verse 1 with groups `[a b c]` and `[d]`, `lps = 2`, live on page 2 (`d`); edit `c` → `C` → `Exact(2)`.
- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::map`. Expected: FAIL.
- [ ] **Step 3: Implement** in `map.rs`. Occurrence lists are tiny (< 50), so the O(n·m) tables are fine.
- [ ] **Step 4: Run** the same command. Expected: PASS (13 tests).
- [ ] **Step 5: Commit** `feat(sections): same_on_screen and position mapping across edits`.

### Task A6: The reconcile decision and Refresh position

**Files:**
- Create: `src/sections/reconcile.rs`

**Interfaces:**
- Consumes: `SlideView`, `same_on_screen`, `Mapped`, `map_position` (A2, A5).
- Produces (Part B's `Live::reconcile` and `Command::Refresh` apply these; this file makes every decision of spec §5.4 steps 2–5 and §5.5 step 3):
  - `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)] #[serde(rename_all = "snake_case")] pub enum ChangeReason { WordsOnScreen, LookOnScreen, PlaceLost, NoSlides }` with `pub fn banner(self) -> &'static str` (the four `strings::BANNER_*`) and `pub fn can_update(self) -> bool` (false for `NoSlides`).
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct LiveChange { pub item_id: Uuid, pub reason: ChangeReason }`
  - `#[derive(Clone, Debug, PartialEq)] pub enum Decision { Same, NotesOnly, Adopt { slide: usize, map: Vec<Option<usize>> }, Banner(ChangeReason) }`
  - `pub fn decide<S: SlideView + PartialEq>(old: &[S], cursor: usize, new: &[S], look_equal: bool, mapper: impl Fn(usize) -> Mapped) -> Decision`. `look_equal` = backgrounds equal and themes equal (computed by the caller). In order: `old == new && look_equal` → `Same`; `new.is_empty()` → `Banner(NoSlides)`; same length, every pair `eq_except_notes`, `look_equal` → `NotesOnly`; `mapper(cursor)` is `Exact(i)`/`Moved(i)` with `same_on_screen(&new[i], &old[cursor])` and `look_equal` → `Adopt { slide: i, map: adopt_map(old.len(), &mapper) }`; `Lost` → `Banner(PlaceLost)`; else `Banner(WordsOnScreen)` when the live slide's text or reference differs from `new[i]`'s **and** `words(old) != words(new)`, otherwise `Banner(LookOnScreen)`.
  - `pub fn words<S: SlideView>(slides: &[S]) -> String`: the text of every `SlideContent::Text`, joined with a space, whitespace collapsed (`split_whitespace().join(" ")`).
  - `pub fn adopt_map(old_len: usize, mapper: impl Fn(usize) -> Mapped) -> Vec<Option<usize>>`: old index → new index (`Lost` → `None`); handed to FEAT-2's `AutomationRuntime::on_adopt(item_id, &map)` (FEAT-2 plan decision 14: the spec's `Automation::on_adopt`, renamed because `Automation` is FEAT-2's persisted root).
  - `pub fn refresh_slide(m: Mapped) -> usize`: `Exact(i) | Moved(i)` → `i`, `Lost` → `0` (D13).

- [ ] **Step 1: Write the failing tests** (`m = |i| map_position(&old, i, &new)`):
  - `identical_is_same`; `identical_slides_but_look_differs_gives_look_reason` (`look_equal = false` → `Banner(LookOnScreen)`; this is the theme/background case of `theme_change_gives_look_reason` and `background_change_gives_look_reason`).
  - `empty_new_gives_no_slides` (+ `ChangeReason::NoSlides.can_update() == false`).
  - `notes_only_change_is_notes_only`.
  - `edit_other_section_adopts`: `V1 C V2`, live on V1 (0), edit V2's text → `Adopt { slide: 0, map: [Some(0), Some(1), Some(2)] }`.
  - `label_only_change_adopts_quietly` (Chorus → Refrain heading, live on it), `occurrence_shift_adopts_quietly` (insert a chorus before the live verse → `Adopt { slide: old+1, .. }`, the `arrangement_change_while_live_keeps_position` case).
  - `edit_live_slide_gives_words_on_screen`.
  - `lines_per_slide_change_gives_look_reason` (same lyrics built with `lps` 2 then 1, live on a page whose text changed → `LookOnScreen`, because `words` are equal).
  - `footer_change_gives_look_reason` (credit footer added to slide 0, live on 0).
  - `section_deleted_gives_place_lost`.
  - `adopt_map_marks_lost_slides_none` (Bridge removed while live on V1 → the bridge's old index maps to `None`).
  - `refresh_slide_lost_is_zero`.
  - `change_reason_json` (`serde_json::to_value(LiveChange{..})["reason"] == "words_on_screen"`) and `banner_texts_match_spec` (`WordsOnScreen.banner()` == "You changed the words that are on screen. Update screen shows them now. Clicking a slide also shows the new version.").
- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::reconcile`. Expected: FAIL.
- [ ] **Step 3: Implement** in `reconcile.rs` (`uuid::Uuid` is already a dependency).
- [ ] **Step 4: Run** the same command. Expected: PASS (15 tests).
- [ ] **Step 5: Commit** `feat(sections): reconcile decision, change reasons and refresh position`.

### Task A7: Arrangement strip helpers, `heading_offset` and `caret_for_occurrence`

**Files:**
- Create: `src/sections/arrangement.rs`

**Interfaces:**
- Consumes: `slides::{parse_sections, section_label}` (read only), `SectionKind` (A1), `resolve_order` (A2).
- Produces (Task B15's `src/ui/dialogs/arrangement.rs` glue only converts these to Slint models; Task B13's song editor calls `caret_for_occurrence`):
  - `#[derive(Clone, Debug, PartialEq)] pub struct ArrChipData { pub label: String, pub short: String, pub kind: SectionKind, pub missing: bool, pub faded: bool }`
  - `pub fn written_order(lyrics: &str) -> Vec<String>`: section labels in written order (the spec's `materialise_written`; duplicates kept).
  - `pub fn chips(lyrics: &str, arrangement: &[String]) -> (Vec<ArrChipData>, Vec<String>, bool)`: chips (when `arrangement` is empty: the written order with `faded: true`, and the bool `written = true`); adds = distinct section labels in written order (first spelling); `missing` = entry matches no section (trimmed, case-insensitive).
  - `pub fn move_entry(v: &mut Vec<String>, from: usize, to: usize)` (product decision 6; out-of-range `from` or `from == to` → no change).
  - `pub fn remove(v: &mut Vec<String>, i: usize)` (out of range → no change).
  - `pub fn ensure_real(v: &mut Vec<String>, lyrics: &str)`: an empty arrangement becomes `written_order(lyrics)`; the glue calls it before the first drag or `+` (spec §3.5).
  - `pub fn add(v: &mut Vec<String>, lyrics: &str, label: &str)` = `ensure_real` then push (repeats allowed).
  - `pub fn heading_offset(lyrics: &str, label: &str, nth: usize) -> Option<usize>` (product decision 5): walks lines with their byte offsets in the original string (CRLF-aware), counts lines where `section_label(line)` equals `label` case-insensitively, and for the `nth` (0-based) returns the byte offset of the next non-empty line after it.
  - `pub fn caret_for_occurrence(lyrics: &str, order: &[String], occurrence: u16) -> Option<usize>`: maps a live occurrence (the live slide's `SectionRef.occurrence`, passed as `DialogKind::SongEditor { caret_section: Some(occurrence) }`) to its written heading. Steps: `labels` = the `parse_sections(lyrics)` labels; `idx = *resolve_order(order, &labels).get(occurrence as usize)?`; `nth` = the number of written sections `j < idx` whose label equals `labels[idx]` (trimmed, case-insensitive). This counts **written** headings, never played repeats, so it is not `SectionRef.ordinal`. Then `heading_offset(lyrics, &labels[idx], nth)`. `order` is the order that built the live slides: B13 passes `song.arrangement` in build step (b), and B14 switches it to `slides::item_order(item, song)` (the override included).

- [ ] **Step 1: Write the failing tests:**
  - `move_entry_left_and_right` (`[a,b,c]`: 0→2 → `[b,c,a]`; 2→0 → `[c,a,b]`), `drop_on_self_does_nothing`, `drop_on_end_zone_moves_last` (0→3 → `[b,c,a]`), `remove_entry`.
  - `first_add_materialises_written_order` (empty + add "Chorus" on `"Verse 1\na\nChorus\nx"` → `[Verse 1, Chorus, Chorus]`).
  - `reset_to_written_order` (`chips(lyrics, &[])` → faded chips, `written == true`).
  - `missing_entries_flagged` (`["Verse 1", "Coda"]` → second chip `missing`, `kind == Ending`, `short == "E"`).
  - `adds_are_distinct_in_written_order`.
  - `heading_offset_finds_nth_heading`: `"Verse 1\na\nChorus\nx\nChorus\ny"`, `("chorus", 1)` → offset of `y`; `("Bridge", 0)` → `None`.
  - `heading_offset_handles_crlf_and_multibyte` (Review Focus 4): `"Verse 1\r\nÉl es 🙏\r\n\r\nChorus\r\nx"`, `("Chorus", 0)` → `Some(i)` with `&lyrics[i..].starts_with("x")` and `lyrics.is_char_boundary(i)`.
  - `heading_offset_none_for_unlabelled` (`"a\n\nb"`, `("Part 2", 0)` → `None`).
  - `caret_for_occurrence_with_duplicate_plain_verses` (Review Focus 1): `"Verse\na\nVerse\nb\nVerse\nc\nVerse\nd"`, empty order: occurrence 0 → offset of `a`, occurrence 2 → offset of `c`, occurrence 3 → offset of `d`.
  - `caret_for_occurrence_with_repeated_arranged_chorus`: `"Verse 1\na\nChorus\nx\nVerse 2\nb"`, order `[Chorus, Verse 1, Chorus, Verse 2]`: occurrences 0 and 2 → offset of `x`; occurrence 3 → offset of `b`; occurrence 9 → `None`.
- [ ] **Step 2: Run** `feat-cargo test --release --lib sections::arrangement`. Expected: FAIL.
- [ ] **Step 3: Implement** in `arrangement.rs`.
- [ ] **Step 4: Run** `feat-cargo test --release --lib sections`. Expected: PASS (all Part A tests, about 82; `sections::arrangement` alone has 13). Then `feat-cargo build --release` to confirm nothing else is affected. Diff against the commit **before Task A1**, never against `main` (the `feat-roadmap` branch already differs from `main` in many `src` files): `git diff --stat "$(git log --format=%H -1 --grep='feat(sections): section kinds, fixed palette')~1" -- src`. Expected: only `src/lib.rs` (+1 line) and `src/sections/*`.
- [ ] **Step 5: Commit** `feat(sections): arrangement editing helpers, heading_offset and caret_for_occurrence`.

---

# PART B — Integration after Plan 3 and Plan 4 merge

> **BLOCKED** until `native-foundation` has Plans 3 and 4 merged and this branch is rebased on it. Part B is written against Plan 3's layout (`src/ui/operator/*.rs`, `src/ui/dialogs/*.rs`, `ui/operator/*.slint`, `ui/dialogs/*.slint`, `ui/theme.slint`, `ui/widgets.slint`, `tests/ui_operator_flows.rs`). Before each task, re-read the named files on the merged branch; if Plan 4 moved something (for example the phone page), apply the same change at its new place and note it in the commit. The test command is `cargo test` (whole suite) unless a task says otherwise; `cargo build --release --bins` must pass after every task.
>
> **Order follows spec §2's build steps, and each step ships on its own:** step (a) = B1–B7 (sections, colours, jump keys, phone chips); step (b) = B8–B13 (reconcile, Refresh, Edit words); step (c) = B14–B15 (`ItemKind::Song.arrangement`, `item_order`, drag-reorder); step (d) = B16 (per-service override UI, **only if Jake accepts D10**); B17 = docs and manual checks. Release after any step's last task: the build passes and every earlier step's flows still work.

## Build step (a): sections, colours, jump keys, phone chips

### Task B1: `Slide.section`, `Slide::new`, `section_keys`, strings move

**Files:**
- Modify: `src/model.rs`, `src/sections/refs.rs` (`SlideView` impl), `src/sections/mod.rs`, plus every file the Step 3 grep finds (on today's branch that includes `src/slides.rs`, `src/live.rs` tests and `src/ui/stage.rs` tests; Plan 3/4 add more)
- Move: `src/sections/strings.rs` → `src/strings/song.rs` (create `src/strings/mod.rs` with `pub mod song;` if no other FEAT has; add `pub mod strings;` to `src/lib.rs` only if missing)

**Interfaces:**
- `Slide` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub section: Option<SectionRef>` (and keeps FEAT-5's `source` if it landed first); `impl Slide { pub fn new(label: impl Into<String>, content: SlideContent) -> Self }`.
- `Settings` gains `#[serde(default = "yes")] pub section_keys: bool` (Default `true`; add `fn yes() -> bool { true }` if missing).
- `impl SlideView for Slide`: `section()` → `self.section.as_ref()`; `eq_except_notes` also compares `source` when FEAT-5 has added it.
- `src/sections/mod.rs`: `pub use crate::strings::song as strings;`.
- `ItemKind::Song.arrangement` is **not** added here; it belongs to build step (c) (Task B14).

- [ ] **Step 1: Write the failing tests** in `src/model.rs`: `old_data_loads_without_slide_sections` (a `data.json` fixture saved from `native-foundation` before this task, `tests/fixtures/feat4-old-data.json`, loads unchanged), `old_settings_default_section_keys_on`, `slide_section_not_serialized_when_none`, `slide_new_has_empty_notes_and_no_section`.
- [ ] **Step 2:** `cargo test model::` → FAIL.
- [ ] **Step 3:** Implement. Then list every struct literal: `grep -rn "[^a-zA-Z]Slide {" src tests` on the merged branch and convert **every** hit to `Slide::new(..)` (set `notes` afterwards where it was non-empty). Re-run the grep; the only remaining hits are the struct definition and `Slide::new` itself. Move the strings file.
- [ ] **Step 4:** `cargo test` passes (including all `sections::` tests, except `model_slide_has_no_section_until_b1`, which this task deletes).
- [ ] **Step 5:** Commit `feat(model): slide sections and section keys setting`.

### Task B2: Song slides carry `SectionRef`

**Files:**
- Modify: `src/slides.rs`, `src/import/easyworship.rs` (calls `song_order`, line ~978 today), `src/ui/dialogs/song_editor.rs` (Plan 3's preview calls `song_slides`), `src/sections/refs.rs` (test kit), plus every other caller the Step 3 grep finds (FEAT-6 print if it landed)

**Interfaces:**
- `pub fn song_order(order: &[String], sections: &[Section]) -> Vec<usize>` delegates to `sections::resolve_order` (labels from `sections`).
- `pub fn song_slides(song: &Song, order: &[String], settings: &Settings) -> Vec<Slide>`: zips with `sections::song_section_refs(&song.lyrics, order, settings.lines_per_slide as usize)` and sets `section`.
- `build_item` passes `&song.arrangement` in this step (Task B14 switches it to `item_order`).

- [ ] **Step 1: Write the failing tests** (spec §9.1 `slides.rs`): `song_slides_tag_section_ref`, `repeated_chorus_gets_distinct_ordinals`, `unnumbered_verses_get_verse_nth`, `slide_labels_unchanged` (the labels of the existing test songs are identical to a fixture captured before this change), `song_order_new_signature_matches_old` (for every existing `slides.rs` test song, `song_order(&song.arrangement, &secs)` equals the old result), `easyworship_import_order_unchanged` (the existing EasyWorship fixture imports the same order).
- [ ] **Step 2:** `cargo test slides::` → FAIL.
- [ ] **Step 3:** Implement. Update every caller: `grep -rn "song_slides\|song_order" src tests` and change each call to the new signatures (`easyworship.rs`: `crate::slides::song_order(&s.arrangement, &secs)`; the song-editor preview: `slides::song_slides(&song, &song.arrangement, &settings)`). Switch `kit::song` to the new `song_slides` and take `section` from each slide (no zip); keep its assertion that the sections equal `song_section_refs(..)` element by element, as a guard that the two stay in step.
- [ ] **Step 4:** `cargo test` passes; the grep shows no call with the old arity.
- [ ] **Step 5:** Commit `feat(slides): tag song slides with sections`.

### Task B3: `Command::Jump` and `LiveItem.song`

**Files:**
- Modify: `src/live.rs`, the `Plan` implementation (`DataPlan::build` in `src/app.rs`)

**Interfaces:**
- `Command::Jump { item_id: Uuid, kind: SectionKind, #[serde(default)] number: Option<u8>, #[serde(default)] occurrence: Option<u16> }`; `relative()` includes `Jump`.
- `pub struct SongRef { pub id: Uuid, pub title: String, pub ccli: Option<String>, pub writers: String, pub copyright: String }`; `LiveItem.song: Option<SongRef>`, filled by `DataPlan::build` for song items.
- Run (spec §5.2): base check as Next → no cursor `Err(strings::NOTHING_LIVE)` → cursor item ≠ `item_id` `Err(strings::LIVE_SONG_CHANGED)` → `sections::jump_target(&self.item.slides, cursor.slide, &JumpTarget{..})`, mapping `JumpError::message()` → `self.go(item, cursor.item_index, target)` with the normal Visual/User commit. Errors never change frame or version.
- Outcome: success is the ordinary `Ok` outcome. The operator's toast is built from the key's own `(kind, number)` (Task A4 `jump_toast`, product decision 4), so the outcome needs no landed-section field and nobody reads `LiveView` after sending.
- **Practice mode (needs FEAT-6):** spec §10 routes `Jump` to the practice engine while practice is on (FEAT-6 §5.5 routing table). If FEAT-6's router exists on the merged branch, add `Command::Jump` to its table with test `jump_in_practice_moves_practice_engine_only`; otherwise leave a `// FEAT-6: practice routing (Jump)` marker at the command dispatch and add the row to FEAT-6's plan.

- [ ] **Step 1: Write the failing tests:** `jump_requires_base`, `jump_queued_before_clear_all_is_stale`, `jump_after_clear_slide_brings_text_back`, `jump_on_non_song_errors_and_keeps_version`, `jump_wrong_item_id_errors`, `duplicate_jump_id_applies_once`, `jump_to_current_slide_bumps_version_frame_equal`, `live_item_song_ref_filled_for_songs`.
- [ ] **Step 2:** `cargo test live::` → FAIL. **Step 3:** implement. **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(live): Jump command to song sections`.

### Task B4: `LiveView.sections` and the phone Jump allowlist

**Files:**
- Modify: `src/app.rs` (`LiveView`, `LiveItemSummary`), `src/server.rs` (`remote_command`)

**Interfaces:**
- `LiveView` gains `pub sections: Vec<OccurrenceView>` (= `sections::occurrence_views(&live.item.slides, Some(cursor.slide))`, always the snapshot); `LiveItemSummary` gains `song_id: Option<Uuid>` from `LiveItem.song`.
- `server.rs remote_command`: allow `Command::Jump { .. }` only with `env.base.is_some()`. Add the row to FEAT-2 §5.11's table if that spec's owner has not.

- [ ] **Step 1: Write the failing tests:** `live_view_lists_sections_with_live_flag`, `live_view_sections_empty_for_non_song`, `live_item_summary_has_song_id`; `server.rs`: `phone_jump_with_base_allowed`, `phone_jump_without_base_rejected`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(app): live sections in LiveView; phone Jump allowed with a base`.

### Task B5: Theme tokens, `SectionChip`, section bar and thumbnail bands

**Files:**
- Create: `ui/operator/section-bar.slint`, `src/ui/operator/sections.rs`
- Modify: `ui/theme.slint`, `ui/widgets.slint`, `ui/operator/slides.slint`, `src/ui/operator/slides.rs`, `src/ui/operator/view.rs` (`OperatorSnapshot` builds the selected item's library slides off the UI thread)

**Interfaces:**
- `Theme`: `sec-verse … sec-other` with the adjusted palette, and `pure function section-color(kind: string) -> color` (`kind` = the serde name, `"pre_chorus"` etc.).
- `SectionChip`, `SectionBar`, `SectionBarChip` exactly as spec §7; `SlideThumb` gains `section-kind`, `section-short` (4 px band + chip; `ThumbKey` unchanged).
- `pub fn bar_chips(snap: &OperatorSnapshot, selected: Option<Uuid>, live: &LiveView, keys_on: bool) -> Vec<SectionBarChip>`: the live item → from `live.sections` with `live` marks and, when `keys_on`, tooltip `strings::chip_key_hint(label, key)`; any other item → `sections::occurrence_views(&library_slides, None)`, tooltip `strings::CHIP_SHOW_HINT`. `pick(o)`: live → `Command::Jump { item_id, kind, number: None, occurrence: Some(o) }` with `base = live.version` (stale → toast `SCREEN_CHANGED`); other → `Command::Show { item_id, slide: first_slide }` (no base).

- [ ] **Step 1: Write the failing tests:** `theme_section_tokens_match_rust_palette` (every `Theme.sec-*` equals `SectionKind::color()`), `bar_chips_follow_arrangement`, `bar_marks_live_occurrence_only_for_live_item`, `key_hints_only_on_live_item`, `chip_click_on_live_item_sends_jump_occurrence`, `chip_click_on_other_item_shows_first_slide`, `thumb_band_kind_matches_slide_section`, `section_bar_hidden_for_non_song`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): section colours on thumbnails and the section bar`.

### Task B6: Jump keys, Settings switch and Help

**Files:**
- Modify: `src/ui/operator/keys.rs`, `src/ui/operator/window.rs`, `ui/dialogs/settings.slint`, `src/ui/dialogs/settings.rs`, the Help rows (Plan 3 Task 20)

**Interfaces:**
- `KeyAction::Jump { kind: SectionKind, number: Option<u8>, key: char }`; `KeyCtx` gains `section_keys: bool`. In `route`, after Enter: not in a text field, no dialog, `section_keys` → `sections::jump_for_key(key, ctrl_or_cmd, alt)`. Check every binding against FEAT-1 §7's key table and FEAT-3's `OS_RESERVED`.
- Handler: nothing live → toast `NOTHING_LIVE`; `live.sections` empty → toast `KEYS_SONGS_ONLY`; else `Ops::cmd(Command::Jump{ item_id, kind, number, occurrence: None })` with `base = live.version`. On an `Ok` outcome toast `sections::jump_toast(kind, number, key)` from the action's own fields (never from `LiveView`); on a `JumpError` text, toast it; stale stays silent.
- Settings → Presenting: `CheckBox` `SETTING_SECTION_KEYS`, `accessible-id: "settings-section-keys"`, hint `SETTING_SECTION_KEYS_HINT`, saved by merge-by-key `settings/save`. Help gains `HELP_DIGITS` and `HELP_LETTERS`.

- [ ] **Step 1: Write the failing tests** (spec §9.2 `keys.rs`): `letters_and_digits_route_to_jump`, `l_routes_to_interlude`, `jump_keys_ignored_in_text_field`, `jump_keys_ignored_with_dialog_open`, `jump_keys_off_in_settings_do_nothing`, `ctrl_or_alt_letter_is_not_a_jump`, `zero_and_unmapped_letters_do_nothing`; flows in `tests/ui_operator_flows.rs`: `c_key_jumps_live_song_to_chorus_and_toasts` (toast "Jumped to Chorus (C)"), `digit_key_toast_names_verse_number` ("Jumped to Verse 2 (2)"), `failed_jump_shows_error_not_jump_toast` (`T` on a song without a Tag → "This song has no Tag." only), `stray_typing_with_no_field_focused_toasts_each_jump`, `stray_typing_on_non_song_live_item_changes_nothing`, `c_key_with_preview_on_other_song_still_acts_on_live`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): section jump keys with toasts and a Settings switch`.

### Task B7: Phone section chips

**Files:**
- Modify: `web/remote.js`, `web/remote.html` (or the phone page's new place after Plan 4)

**Interfaces:**
- A `const STR = {…}` object at the top of `remote.js` holding the phone's FEAT-4 texts (copied from `strings::song`, a test compares them if Plan 4 kept a JS harness).
- A scrollable chip row from `LiveView.sections` (colours from `OccurrenceView.color`, short + label, live highlighted, hidden when empty); a tap sends `{cmd: "jump", item_id, kind, occurrence}` with the phone's `base`; a stale result resyncs and toasts "The screen changed — try again."; a `JumpError` text is shown as the toast.

- [ ] **Step 1: Write the failing tests:** if Plan 4 kept a remote JS test harness, add `phone_chips_render_from_live_sections`, `phone_chip_tap_sends_jump_with_base`, `phone_stale_jump_resyncs_and_toasts`, `phone_strings_match_rust`; otherwise add these as manual checks 1–3 of Task B17.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes (and the JS harness, if any).
- [ ] **Step 5:** Commit `feat(phone): section chips jump the live song`. **Build step (a) can ship here.**

## Build step (b): reconcile, Refresh, Edit words

### Task B8: `Live.changed` and `Live::reconcile` (decide, banner, notes-only)

**Files:**
- Modify: `src/live.rs`

**Interfaces:**
- `pub changed: Option<LiveChange>` on `Live` and on its revert `Snapshot`; `pub use crate::sections::{LiveChange, ChangeReason};`. `go` clears `changed`.
- `pub fn reconcile(&mut self, plan: &dyn Plan) -> bool` (spec §5.4): nothing live → false; live scripture with a pending plan → false; `plan.build(item_id)` `None` → clear `changed` if it named this item; else `sections::decide(&old, cursor.slide, &new.slides, look_equal, mapper)` where `look_equal = new.background == old.background && new.theme == old.theme` and `mapper` is `map_position` for every kind in this task (B9 adds the scripture mapper). Apply: `Same` → clear `changed` (this is how an edit undone by hand clears the banner, spec §3.4 step 5); `NotesOnly` → copy slides, `commit_change(Change::None)`, clear; `Banner(r)` → `changed = Some(LiveChange{item_id, reason: r})`, nothing else. Until B9, `Adopt` is applied as `Banner(LookOnScreen)` (safe: nothing on screen changes; B9 replaces it). Returns whether live state changed.
- **If FEAT-2's `Change`/`commit_change` has not landed:** implement only the parts FEAT-4 needs as FEAT-2 §5.2 defines them (`enum Change { None, Silent, Overlay, Visual }`, a `commit_change` where Silent bumps `version` only) in `live.rs`; FEAT-2's plan then replaces them.

- [ ] **Step 1: Write the failing tests** (spec §9.1 `live.rs`): `notes_only_change_adopts_with_no_version_change`, `theme_change_gives_look_reason`, `edit_live_slide_sets_words_on_screen_and_keeps_frame_and_version`, `lines_per_slide_change_gives_look_reason`, `footer_change_gives_look_reason`, `background_change_gives_look_reason`, `undoing_edit_by_hand_clears_changed` (edit the live slide → `WordsOnScreen`; restore the old lyrics → `reconcile` → `changed == None`, frame and version unchanged), `take_live_other_item_clears_changed`, `removed_live_item_has_no_banner`, `service_switch_clears_banner`, `pending_scripture_plan_skips_reconcile`.
- [ ] **Step 2:** `cargo test live::` → FAIL. **Step 3:** implement. **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(live): reconcile the live item with the library (change banner)`.

### Task B9: Quiet adoption, `on_adopt` and per-kind mappers

**Files:**
- Modify: `src/live.rs`, `src/room.rs` (test only, unless it fails)

**Interfaces:**
- `Decision::Adopt{slide, map}` → copy **only** `slides`, `cursor.slide = slide`, clear `changed`, `app.automation.lock().on_adopt(item_id, &map)` (FEAT-2's `AutomationRuntime::on_adopt`, its plan decision 14; the spec calls it `Automation::on_adopt`), `commit_change(Change::Silent)`. If FEAT-2's `AutomationRuntime::on_adopt` has not landed, skip the call behind a `// FEAT-2: on_adopt` marker.
- Mappers: songs → `map_position`; scripture → `slides::index_for_source` (FEAT-5) wrapped as `Mapped::Exact(i)` when `i == old index` and `same_on_screen`, else `Mapped::Moved(i)`, `None` → `Lost`; media and other kinds → `map_position` (by index, the non-song branch).
- `room.rs`: a new version with an unchanged frame commits without a prepare (spec §5.4 "Silent").

- [ ] **Step 1: Write the failing tests:** `edit_other_section_adopts_quietly_frame_unchanged`, `adoption_makes_in_flight_next_stale`, `adoption_copies_only_slides`, `label_only_change_adopts_quietly`, `occurrence_shift_adopts_quietly`, `arrangement_change_while_live_keeps_position`, `quiet_adoption_calls_on_adopt`, `scripture_edit_reconciles_by_source`, `media_item_edit_reconciles_by_index`, `unchanged_frame_commit_clears_pending`; `src/room.rs`: `unchanged_frame_new_version_commits_without_prepare`.
- [ ] **Step 2:** `cargo test live:: room::` → FAIL. **Step 3:** implement (remove B8's Adopt stub). **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(live): quiet adoption of library edits at the same place`.

### Task B10: `Command::Refresh`

**Files:**
- Modify: `src/live.rs`, `src/server.rs` (test only: Refresh stays rejected)

**Interfaces:**
- `Command::Refresh { item_id: Uuid }` (no base; not relative). Spec §5.5: wrong item → `Err(strings::NO_LONGER_LIVE)`; `plan.build` missing or empty → `Err(strings::NO_SLIDES_NOW)`, `changed` kept; position = `sections::refresh_slide(mapper(cursor.slide))`; slide layer up → `go(new, cursor.item_index, position)` as Visual/User, `changed = None`; slide layer down → replace the whole snapshot, set the cursor, `changed = None`, `commit_change(Change::Silent)`, frame untouched, and the outcome carries `strings::SAVED_TEXT_CLEARED` as its toast text.
- **Practice mode (needs FEAT-6):** as in B3, `Refresh` goes to the practice engine while practice is on; add it to FEAT-6's routing table with `refresh_in_practice_updates_practice_engine_only`, or leave a `// FEAT-6: practice routing (Refresh)` marker.

- [ ] **Step 1: Write the failing tests:** `refresh_shows_new_words_at_same_position`, `refresh_lost_place_goes_to_slide_zero`, `refresh_with_no_slides_errors_and_keeps_changed`, `refresh_wrong_item_errors`, `refresh_while_cleared_keeps_clear`, `refresh_while_logo_keeps_logo`, `failed_refresh_prepare_reverts_snapshot_and_banner`, `refresh_keeps_background_video_playing` (same background → no new background instance); `server.rs`: `phone_refresh_rejected`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(live): Refresh command applies library changes at the same place`.

### Task B11: App reconcile callers and `LiveView.changed`

**Files:**
- Modify: `src/app.rs`

**Interfaces:**
- `App::edit` calls `live.reconcile(&plan)` after the mutation whether or not the save succeeded (lock data → live), then broadcasts live if it returned true; same on `Update::PlansReady`, in `backup/restore` after `*app.data` is replaced, and in `App::room_event` right after `Live::report` reverts on `RoomEvent::Failed` (`room_event` takes the data lock before the live lock).
- `LiveView` gains `pub changed: Option<LiveChange>`.

- [ ] **Step 1: Write the failing tests:** `edit_reconciles_live_under_data_then_live_lock`, `edit_reconciles_even_when_save_fails`, `restore_while_live_reconciles`, `failed_prepare_after_adoption_rechecks_library`, `plans_ready_reconciles`, `phone_chips_follow_snapshot_while_changed`, `settings_lines_per_slide_while_live_gives_look_banner`, `live_view_carries_changed`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(app): reconcile the live item after every data change`.

### Task B12: Live column — section line, Edit words button, change banner, Update screen; up next; stage label

**Files:**
- Modify: `ui/operator/live.slint`, `src/ui/operator/live.rs`, `ui/stage.slint` (or FEAT-3's stage layout if it has landed), `src/ui/stage.rs`, `src/ui/dialogs/mod.rs` (or wherever Plan 3 defines `DialogKind`)

**Interfaces:**
- Under the preview, one row: `sections::live_section_line` text with its colour chip, and **Edit words** (`Btn small`, `accessible-id: "live-edit-words"`, visible when `live.item.song` is set) at its right end. Then the banner (`Theme.notice-bg/notice-border`, `accessible-id: "live-changed"`, text `reason.banner()`, at most two lines) with **Update screen** (`Btn primary small`, `live-update-screen`, hidden when `!reason.can_update()`), in FEAT-1 §7.2's live-column order.
- Up next shows the next cue's section chip (`SectionChip` from the next slide's `SectionRef`, hidden when it has none or the next cue is another item without sections).
- `DialogKind::SongEditor` gains `caret_section: Option<u16>` (default `None` at every existing construction site). Edit words opens `DialogKind::SongEditor { song_id, add_after: false, caret_section: Some(occurrence) }`, `occurrence` = the live slide's `SectionRef.occurrence`. Update screen sends `Command::Refresh { item_id }` and toasts the outcome's text.
- Stage: one label line above the current text: `SectionRef.label` plus " (p/n)" when `pages > 1`, colour from `SectionKind::color()` passed as a `color` property.

- [ ] **Step 1: Write the failing tests:** `section_line_shows_live_section`, `edit_words_hidden_for_non_song`, `edit_words_opens_editor_with_live_occurrence`, `banner_shown_with_reason_text`, `update_button_hidden_for_no_slides`, `up_next_shows_next_section_chip` (live on V1's last slide with the chorus next → Up next chip `C` in the chorus colour; a non-song next cue → no chip), flows `save_changes_words_on_screen_shows_banner`, `update_screen_clears_banner_and_shows_new_words`, `save_unrelated_section_shows_no_banner`; `stage_label_follows_live_section`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): live section line, Edit words and the change banner`.

### Task B13: Song editor live editing (caret, live banner, Save and show now)

**Files:**
- Modify: `ui/dialogs/song-editor.slint`, `src/ui/dialogs/song_editor.rs`

**Interfaces:**
- `in property <bool> live-song` = `Some(song.id) == live.item.song.map(|s| s.id)`, recomputed on every live update while open; banner `LIVE_EDIT_BANNER`; footer Cancel, Save, and while live **Save and show now** (primary) = `song/save` then, on success only, `Ops::cmd(Refresh)`.
- Caret: when `caret_section` is `Some(o)`, `sections::caret_for_occurrence(&song.lyrics, &song.arrangement, o)` (step (b) order; B14 switches to `slides::item_order(live_item, song)`); `Some(off)` → `set-selection-offsets(off, off)`, `None` → 0; focus the Lyrics box.

- [ ] **Step 1: Write the failing tests:** flows `edit_words_opens_editor_at_live_section`, `edit_words_on_second_plain_verse_puts_caret_in_verse_two` (four plain `Verse` headings, live on the third → caret at its first lyric line), `editor_from_library_shows_live_banner_for_live_song`, `save_and_show_now_updates_live`, `save_and_show_now_failed_save_sends_no_refresh`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): live-safe song editing with Save and show now`. **Build step (b) can ship here.**

## Build step (c): drag-reorder, `ItemKind::Song.arrangement`, `item_order`

### Task B14: `ItemKind::Song.arrangement` and `slides::item_order`

**Files:**
- Modify: `src/model.rs`, `src/slides.rs`, `src/app.rs` (`ItemKind::Song { song_id, .. }` patterns), `src/ui/dialogs/song_editor.rs` (caret order), plus every `ItemKind::Song {` pattern `grep -rn "ItemKind::Song {" src tests` finds

**Interfaces:**
- `ItemKind::Song { song_id: Uuid, #[serde(default, skip_serializing_if = "Option::is_none")] arrangement: Option<Vec<String>> }`. FEAT-7's Planning Center import writes it; this ships whatever D10 decides.
- `pub fn item_order<'a>(item: &'a ServiceItem, song: &'a Song) -> &'a [String]` via `sections::pick_order(item.arrangement.as_deref(), &song.arrangement)`. `build_item` (and FEAT-6 print, if it landed) uses it. B13's caret switches to `item_order(live_item, song)`.

- [ ] **Step 1: Write the failing tests:** `old_data_without_item_arrangement_loads` (fixture `tests/fixtures/feat4-old-data.json` from B1: the song item's `arrangement` is `None`), `item_arrangement_none_is_not_serialized` (no `"arrangement"` key), `item_arrangement_round_trips`, `item_order_prefers_item_override`, `empty_override_list_means_written_order` (an item with `Some(vec![])` builds the written order although the song has its own order), `build_item_uses_item_override`, `edit_words_caret_follows_item_override`.
- [ ] **Step 2:** `cargo test model:: slides::` → FAIL. **Step 3:** implement; add `..` to every `ItemKind::Song { song_id }` pattern. **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(model): per-service song order field and item_order`.

### Task B15: Arrangement strip in the song editor

**Files:**
- Create: `ui/dialogs/arrangement.slint`, `src/ui/dialogs/arrangement.rs`
- Modify: `ui/dialogs/song-editor.slint`, `src/ui/dialogs/song_editor.rs`

**Interfaces:**
- `ArrangementStrip`, `ArrChip` exactly as spec §7 (DragArea data `arr:<index>`, one DropArea per chip plus an end zone, own `FocusScope` for Ctrl+←/→, ←/→, Delete/Backspace; dragged chip 40 % opacity, 2 px accent drop bar; missing chips grey dashed with tooltip `NOT_IN_LYRICS`; faded chips + `WRITTEN_ORDER_HINT` while written; **Use written order** disabled while empty). `arrangement.rs` maps `sections::chips` to the model and calls `ensure_real`/`move_entry`/`remove`/`add`; the preview refreshes 250 ms after a change (Plan 3 behaviour).
- Song editor: the Order `LineEdit` and `order_warnings` are removed; the strip edits `song.arrangement`.

- [ ] **Step 1: Write the failing tests** (spec §9.2 `arrangement.rs` UI part): `ctrl_arrow_moves_selected_chip`, `delete_key_removes_selected_chip`, `drag_chip_reorders` (`mock_drag` where it works, otherwise the `move` callback), `use_written_order_disabled_when_empty`, `missing_chip_shows_not_in_lyrics_tooltip`, flow `reorder_while_live_adopts_quietly_after_save`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): drag-to-reorder arrangement strip`. **Build step (c) can ship here.**

## Build step (d): per-service override UI — only if Jake accepts D10

### Task B16: Item edit order, Custom order badge and header

**Files:**
- Modify: `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs`, `ui/operator/services.slint`, `src/ui/operator/services.rs`, `ui/operator/slides.slint` (header line / ⋯ menu below 1500 px), `src/ui/operator/slides.rs`

**Interfaces:**
- **Skip this whole task if Jake declines D10**; the field and `item_order` from B14 stay for FEAT-7.
- Item edit → Words: `Field` `ORDER_FOR_SERVICE` with `CheckBox` `USE_USUAL_ORDER` (checked by default; unchecking copies `item_order` or the written order into the strip) and the strip (disabled while checked); Save sends merge-by-key `service/update` with `arrangement` as a list, or explicit `null` to clear; **Make this the song's usual order** = `song/save` with the strip, then `service/update` with `arrangement: null`, toast `SAVED_AS_USUAL`.
- Service row `custom-order` → `Chip` `CUSTOM_ORDER` (`custom-order-<n>`); slides header `CUSTOM_ORDER_HEADER` in `Theme.muted`; below 1500 px it moves into the header's ⋯ menu.
- FEAT-6 copy rule (Copy last week / templates drop `arrangement`, Copy this service keeps it) is FEAT-6's to implement; add a note to FEAT-6's plan if it exists.

- [ ] **Step 1: Write the failing tests:** `override_saved_on_item_only`, `use_usual_order_clears_override`, `make_usual_order_saves_song_and_clears_override`, `custom_order_badge_shows_on_service_row`, `custom_order_header_moves_to_menu_below_1500px`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): per-service song order (Custom order)`.

## Docs and checks

### Task B17: Spec updates, PROJECT_PLAN and manual checks

**Files:**
- Modify: `docs/superpowers/specs/2026-11-01-feat4-song-workflow-design.md`, `PROJECT_PLAN.md`, `docs/superpowers/checks/2026-native-checks.md`

- [ ] **Step 1: Spec update.** §6.1 table → the adjusted palette (Global Constraints). §5.1: replace the parenthetical "(so "Bridge (2/3)" labels and `page` agree)" with "(slide labels stay per blank-line group as today, so for a section split by a blank line the label and `page` differ; the stage label uses `page/pages`)" — product decision 7. §11: add rows for Jake on product decisions 1 (`V` = next verse ahead), 4 (toast = kind name + digit number, as §3.2 already says; confirm) and 7, and record D10's answer.
- [ ] **Step 2: Manual checks** (spec §9.3, on the church laptop with the projector and a phone): all ten items, plus the phone checks from B7 if no JS harness exists, plus practice-mode Jump/Refresh if FEAT-6 has landed; record results in `docs/superpowers/checks/2026-native-checks.md` as "FEAT-4".
- [ ] **Step 3:** Update `PROJECT_PLAN.md` FEAT-4 status (which build steps shipped). Commit `docs: FEAT-4 palette, label note, decisions and checks`.
