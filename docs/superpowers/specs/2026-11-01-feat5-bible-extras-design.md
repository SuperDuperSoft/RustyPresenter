# FEAT-5 — Bible Extras: Keyword Search, Two Translations, Slide Breaks — Design Spec

**Date:** November 1, 2026 (revision 2, after review)
**Status:** Draft for review (written while Jake was away; see "Decisions for Jake to review")
**Roadmap:** `PROJECT_PLAN.md` §10b, FEAT-5. Depends on Plan 3 (native operator). Works before or after FEAT-1 (§10).
**Controls:** v1 spec §5.4 (command ids), §5.5 (responsiveness), §6 (layers, clear matrix), §7 (navigation), §8 (Bible). This spec adds to §8; it does not change any v1 rule. It builds the v1 §7.2 "Live item changed — Take Live to update." notice, which Plan 3 lists as not built.

## 1. Purpose and who it's for

Volunteers at a typical small church put scripture on screen in three situations the v1 Bible box does not cover well:

1. **"I know the words, not the reference."** The pastor says "be still and know that I am God". The volunteer needs to find Psalm 46:10 in seconds, without leaving the app. (Search matches the words as typed and their longer forms: `know` finds knoweth, but `being` does not find be.)
2. **Two translations at once.** A pastor who preaches from the NIV but wants the KJV wording beside it, or a study series comparing wordings. ProPresenter and EasyWorship both offer this. All six supported translations are English today; a bilingual congregation can use this once a non-English translation provider exists (the provider owns versification mapping, §5.1).
3. **"That slide break is in the wrong place."** Automatic splitting is good but not perfect: a verse that belongs with the next one ends up alone, or a long passage needs a break where the pastor pauses. Today the only control is "Verses per slide".

The people using this are non-technical volunteers on old laptops. Every flow below works with the keyboard alone, never touches the room output until the volunteer shows something, and explains any problem in plain words.

## 2. Scope

**In:**

- Keyword search across a whole translation, with an Old/New Testament and single-book filter, best-match ranking, a context view of the chapter, and adding a hit (or a range around it) to the service.
- Searching from the Bible box: typing words instead of a reference opens the search; `Ctrl+Enter` always searches.
- A second translation on a scripture item, shown **side by side** (default) or **one above the other**, with both copyright notices.
- A church default for the second translation (off by default) and a per-item choice.
- Choosing slide breaks per scripture item: force a new slide before a verse, keep two verses together, reset to automatic. A visual editor with live slide thumbnails.
- Measured fitting: automatic breaks are decided by the real slide layout (src/render), as v1 §8.3 requires, instead of today's 330-character estimate. Measuring runs on a planner worker, never in the command queue (§5.5).
- Stable verse identity (`VerseId`) on stored verses and on generated slides, so breaks survive translation changes and re-taking an edited live item lands on the right slide (v1 §8.4).
- The "Live item changed — Take Live to update." notice (v1 §7.2) for every item kind.

**Out:**

- Translation sources, licensing, downloads, API keys and the `src/bible.rs` provider plugins. Another session owns those. This spec designs against an abstract provider (§5.1).
- Search on the phone remote. The remote allowlist does not change.
- Manual breaks in the middle of a verse. Breaks stay on verse boundaries (v1 §8.4). Automatic mid-verse splitting of over-long verses stays.
- More than two translations on one slide. Two translations on the stage display (§3.2).
- Topic/concordance search (Strong's numbers, cross references), fuzzy spelling correction, stemming beyond prefix matching.
- Word highlighting inside search result snippets (the hit verse is highlighted as a row instead; §8.1).
- Lower-third/livestream formatting of scripture (FEAT-7 Looks decide what that output shows).

## 3. User experience

### 3.1 Find a verse by its words (operator)

1. The volunteer presses `Ctrl+B`, types `be still and know` and presses `Enter`.
2. The Bible box classifies the text (`classify_bible_input`, §5.6). It is words, not a reference, so the **Find a verse** dialog opens with the words in its search box and results already showing. Nothing is added to the service and the room is untouched.
3. Results list (left): `Psalm 46:10` in bold, and below it the verse text, trimmed to about 160 characters around the first matched word with "…". The header says "3 verses found in KJV" (or "Showing the first 200 of 1,240 verses — add more words to narrow it down"). Results are ordered best match first (§5.2); a small toggle **Best match / Bible order** sits beside the header (remembered for the session).
4. Results update as the volunteer types (250 ms after the last key).
5. Keys in the search box: `Enter` runs any pending search at once (cancelling the debounce), waits for it, then moves focus to the first result. It never adds from the box. `↓` does the same without forcing a pending search (if a search is pending, focus moves when it arrives). With no results, `Enter` and `↓` do nothing.
6. In the list, `↑`/`↓` move through results. The context view (right) shows the whole chapter of the highlighted result, scrolled to the hit verse, which is selected and tinted with the accent colour.
7. Widening the selection: click a verse to select it alone, `Shift`+click (or `Shift+↑`/`Shift+↓` in the context view, `Tab` moves focus there) to extend the range. The range stays inside the shown chapter: `Shift+↓` on the last verse and `Shift+↑` on the first do nothing. The primary button always names what will be added: **Add Psalm 46:10–11**.
8. `Enter` (in the list or the context view) or the button adds that passage: in the **dialog's** translation, plus the church second translation when one is set and set up (§3.2). It is inserted after the live item (or at the end when nothing is live), selected and armed, dialog closed, toast "Psalm 46:10–11 added — press Enter to show it". A second `Enter` takes it live (v1 §8.1 flow, unchanged).
9. Filters in the dialog head: translation drop-down (defaults to the Bible box translation; lists only translations that are set up, others greyed with the Plan 3 reason), and a scope segmented control **Whole Bible / Old Testament / New Testament / One book**; "One book" shows a book drop-down. Scope is remembered for the session only.
10. `Esc` closes the dialog. F1–F5 still clear the room (Plan 3 Task 10 capture rule).

Other ways in: the Add dialog's Bible tab gets a **Search by words…** link that opens the same dialog with whatever is in its reference box; Enter in that tab's box classifies the text exactly as the Bible box does.

Search rules, written for the Help tab:

- Every word must appear in the verse (`love neighbour` finds verses with both).
- Words match their beginnings: `love` finds love, loved, loveth, lovest, loving. Words shorter than 3 letters must match exactly.
- Put words in quotes for an exact phrase: `"be still"`.
- Capitals and punctuation do not matter. `lord` finds LORD.
- Best matches come first: verses with your exact words in order, then verses where the words are close together, then the rest in Bible order. Switch to **Bible order** to see Genesis first.
- `Ctrl+Enter` in the Bible box always searches, even for a book name like "Job".

### 3.2 Show two translations

**Church default.** Settings → Bible gets a new row: **Second translation** drop-down: `None` (default), or any translation that is set up and differs from the main one; others are greyed with the Plan 3 "isn't set up yet" reason. Under it, **Layout**: **Side by side** (default) / **One above the other**, with a live preview thumbnail of John 3:16–17 using the church theme. When the resolved theme uses the lower-third position, **Side by side** is disabled with the note "Lower-third looks always stack two translations." These controls apply at once through `settings/bible` (like the translation chips); they are not part of Settings Save (§5.3).

**In the Bible box.** When a second translation is set, a second chip appears after the translation chips: `+ NIV` with an X. Clicking the X turns the second translation off for the church (saved, like the main chip). When it is off, a small `+` button appears instead; it opens a menu "Also show…" listing set-up translations other than the main one (others greyed with the reason). `Enter` looks up both translations, adds one item, arms it.

- If the saved second translation is no longer set up (licence lapsed, offline provider), the chip shows in the `off` style with the Plan 3 tooltip, and `Enter` adds the **main translation only**, with a warning toast: "NIV isn't set up, so only KJV was added. Turn off the second translation with the X." The main translation is never replaced, so this is not a silent substitution (v1 §8.2).
- If the volunteer picks a main translation equal to the second (clicks the NIV chip or presses `Alt+1` while the second is NIV), the core turns the second off in the same `settings/translation` action and the UI shows the toast "Second translation turned off (same as main)."

**Per item.** Item edit (Plan 3 Task 18) for a scripture item gains:

- **Translation** drop-down (the main one; set-up translations only).
- **Second translation** drop-down (`None` + set-up translations other than the main).
- **Layout** segmented (Side by side / One above the other), shown only when a second translation is chosen; Side by side is disabled with the lower-third note when the item's resolved theme is lower-third.

Changing a translation in Item edit looks the passage up at once (off the UI thread) and updates the dialog's preview. If the lookup fails, the drop-down returns to its previous value and an error toast shows the core's message. Nothing changes in the service until **Save**.

**On screen (room).** Side by side: two columns of equal width, each headed by its translation code (`KJV`, `NIV`) in the reference style; verse numbers as superscripts in both; one shared font size; text left-aligned (as scripture is today). The reference line at the bottom right reads `John 3:16–17 (KJV · NIV)`; under it, a small attribution line carries the copyright notices of both translations joined with " · " (empty notices, such as KJV's, are left out; no line when both are empty). One above the other: main on top, a thin divider, the second below, each with its code at the left, text left-aligned.

**Stage (foldback).** The stage shows the main translation only (FEAT-3 receives `SlideLayer::Text.text`, which is the main translation). `SlideLayer::Text.second` is available to a future stage cell; FEAT-3 needs no work for FEAT-5.

**Phone remote.** Current/next cue text shows the main translation only. Nothing else changes.

### 3.3 Choose where the slides break

1. The volunteer selects a scripture item. The slides grid header (Plan 3 Task 7) shows a new button **Slide breaks** for scripture items. Item edit also has a button **Choose slide breaks…** (it saves the item edits first, then opens the breaks editor).
2. The **Slide breaks** dialog (wide) shows on the left the verse list: every verse of either translation (the union, §5.4), each row with its verse number and the start of its text (main translation, or the second in italics when only the second has it). Between every pair of verses is a **gap line** drawn from `gap_state` (§5.4):
   - **Auto**: dashed accent line, "New slide (automatic)".
   - **Added**: solid accent line, "New slide (you added)".
   - **Kept**: faint line with a link icon, "Kept together".
   - **None**: faint line that shows "+ New slide here" on hover/focus.
3. Clicking a gap (or `Space` when it has focus; `↑`/`↓` move between gaps): None → Added → None; Auto → Kept → Auto; an Added gap that the planner would also break on returns to Auto when clicked.
4. The right side shows the resulting slides as thumbnails, rendered with the item's resolved theme at the planning canvas (§5.4), updated after each click (planned on a worker; a spinner shows on the thumbnails meanwhile). A slide that will be split inside a verse has a warning chip "Too long — will split inside a verse", and when the click just made such a slide, an inline note appears under the gap list (no confirm box): "Keeping these together makes a slide too long, so it will split inside a verse."
5. **Verses per slide** (Automatic / 1–4) is here as well (it stays in Item edit too). Added breaks and kept-together marks work with any setting.
6. Footer: **Reset to automatic** (removes all marks, asks nothing — Cancel undoes it), **Cancel**, **Save**.
7. **Save** sends `service/update` with the edited item. Kept marks on gaps that are no longer automatic are dropped before sending. If the item is live, the room does not change; the Live column shows "Live item changed — Take Live to update." (§5.5). Re-taking it (Take Live or `Enter` on the live item) shows the slide that contains the same verse as the one on screen.
8. If the item was removed meanwhile, Save shows the core's toast "That item is gone." and the dialog stays open (Plan 3 Task 18 behaviour).

## 4. Data model

All new fields have serde defaults; existing `data.json` files load unchanged. No `format` bump.

```rust
// src/model.rs

/// Canonical verse identity in the English (KJV-style) versification shared by
/// all six supported translations. `book` is the 0-based index into bible::BOOKS.
/// Serialized as [book, chapter, verse] to keep data.json small.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerseId(pub u8, pub u16, pub u16);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VerseText {
    pub reference: String,
    pub text: String,
    /// Filled by lookups. Old data has none; Data::fill_verse_ids() derives it from
    /// `reference` (bible::parse_verse_label). Unparseable labels stay None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<VerseId>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParallelLayout { #[default] SideBySide, Stacked }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SecondText {
    pub translation: String,
    pub verses: Vec<VerseText>,
}

/// Manual slide breaks, pinned to verse boundaries (v1 §8.4). A mark on verse v
/// refers to the gap before v.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[serde(default)]
pub struct VerseBreaks {
    /// Always start a new slide before these verses.
    pub before: BTreeSet<VerseId>,
    /// Do not start an automatic new slide before these verses.
    pub join: BTreeSet<VerseId>,
}
impl VerseBreaks {
    pub fn is_empty(&self) -> bool { self.before.is_empty() && self.join.is_empty() }
}

pub enum ItemKind {
    Scripture {
        reference: String,
        translation: String,
        verses: Vec<VerseText>,
        #[serde(default)] per_slide: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")] second: Option<SecondText>,
        #[serde(default)] layout: ParallelLayout,
        #[serde(default, skip_serializing_if = "VerseBreaks::is_empty")] breaks: VerseBreaks,
    },
    // other kinds unchanged
}

/// Where a generated slide's text comes from (scripture only for now).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlideSource {
    pub first: VerseId,
    pub last: VerseId,
    /// 0 = whole verse(s) or the first part of a verse split across slides.
    pub part: u8,
}

pub struct Slide {
    pub label: String,
    pub content: SlideContent,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub source: Option<SlideSource>,
}

pub enum SlideContent {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")] footer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")] reference: Option<String>,
        /// Second translation for a parallel scripture slide.
        #[serde(default, skip_serializing_if = "Option::is_none")] second: Option<ParallelText>,
    },
    // others unchanged
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParallelText {
    pub main_code: String,     // "KJV"
    pub second_code: String,   // "NIV"
    pub text: String,          // second translation's text for this slide
    pub layout: ParallelLayout,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct BibleSettings {
    /// None = one translation (default).
    pub second_translation: Option<String>,
    pub layout: ParallelLayout,
}

pub struct DisplayRoles {
    // existing room / stage fields unchanged
    /// Last size the room window opened at, in pixels. None = never opened (plan at 1920×1080).
    #[serde(default)] pub room_size: Option<[u32; 2]>,
}

pub struct Settings {
    // existing fields unchanged
    #[serde(default)] pub bible: BibleSettings,
}
```

For a parallel slide, `SlideContent::Text.footer` carries the joined copyright notices (§3.2); for a single-translation slide it carries that translation's notice (empty for KJV, so `None`). `live::SlideLayer::Text` gains `second: Option<ParallelText>` (default `None`), and `render::cache::RenderKey` gains the same field, so a parallel slide caches separately. FEAT-1's rich text runs, when they land, replace `text: String` with runs in `SlideContent::Text`; scripture produces plain runs (one per verse-number superscript and per verse) and `ParallelText.text` follows the same representation.

**Migration.**

- `Data::fill_verse_ids()` parses each `VerseText.reference` (`"John 3:16"`, `"Psalm 23:1"`) with the new `bible::parse_verse_label` and fills `id`. Nothing else is rewritten. It runs in every path that replaces or adds `Data`: `store.load`, `store.restore` (backup restore), and any import that brings scripture items (today none; the EasyWorship importer in `src/import/` brings songs only, and must call it if it ever imports scripture). One helper, `Data::after_load(&mut self)`, holds all load fix-ups so new paths cannot forget it.
- `ParallelLayout` serializes as `"side_by_side"` / `"stacked"`.
- **Automatic breaks may move after the upgrade.** Existing scripture items are re-planned by measurement instead of the 330-character estimate, so a service planned before the upgrade may show more or fewer slides. The first time the app starts on the new version with scripture in the service, a one-time toast says "Bible slides are now fitted to the screen, so some passages may break in different places." (flag `Data.notes_seen: BTreeSet<String>`, key `"feat5-measured-breaks"`).
- **Rollback.** An older RustyPresenter that saves `data.json` drops `second`, `layout`, `breaks`, `bible` and `room_size` (its serde ignores unknown fields on load and does not write them back). Rolling back loses second translations and manual breaks; the verses themselves stay. The release notes say so.

**Validation in the core** (`service/add`, `service/update`): `second.translation` must differ from `translation` ("Pick a different second translation."); `breaks` entries that are not verses of the item (union of both translations) are dropped; a verse in both `before` and `join` keeps only `before`; `before` on the first verse is dropped.

## 5. Core behaviour

### 5.1 Translation provider contract (abstract, shared with FEAT-6/FEAT-7)

FEAT-5, FEAT-6 (print) and FEAT-7 (Planning Center import) use one trait name and method list. The provider session may rename modules; the capabilities stay.

```rust
pub enum SearchAccess {
    /// The whole text is available locally; FEAT-5 builds its own index.
    AllVerses,
    /// The source has its own search (e.g. a licensed online API); FEAT-5 forwards queries.
    Provider,
    /// No search for this translation.
    None,
}

pub trait TranslationProvider: Send + Sync {
    fn code(&self) -> &str;                                  // "KJV"
    fn display_name(&self) -> &str;                          // "King James Version"
    /// Attribution text required on screen and in print; "" when none (KJV).
    fn copyright_notice(&self) -> &str;
    fn is_ready(&self) -> Result<(), String>;                // Err = the Plan 3 "isn't set up yet…" reason
    fn passage(&self, r: &Reference) -> Result<Vec<VerseText>, String>; // ids filled
    fn chapter(&self, book: u8, chapter: u16) -> Result<Vec<VerseText>, String>;
    fn search_access(&self) -> SearchAccess;
    /// Only for AllVerses. Calls f for every verse in Bible order; stops early when
    /// `cancel` is set and returns Err("cancelled").
    fn for_each_verse(&self, cancel: &AtomicBool, f: &mut dyn FnMut(VerseId, &str)) -> Result<(), String>;
    /// Only for Provider. At most `limit` hits, best match first when the source ranks.
    fn search(&self, q: &SearchQuery, limit: usize) -> Result<SearchResults, String>;
}
```

- Verses are keyed by the shared English versification. A provider whose edition differs maps to it; that mapping is the provider's job.
- `AllVerses` only when **every** verse is locally available. A translation that is "Prepared offline" for some passages only (v1 §8.5 readiness) reports `Provider` (if the source can search) or `None`, never `AllVerses`, so search is never silently incomplete.
- Today's bundled KJV implements `AllVerses` with an empty notice. The core reaches providers through `bible::provider(code) -> Result<&dyn TranslationProvider, String>`.
- **Storage.** A scripture item stores the verse text of both translations in `data.json` and backups, exactly as v1 stores the main translation today. Whether a provider permits that is the provider session's decision (v1 §8.5 "permitted local storage"); the second translation is under the same terms as the main one. The search index is never written to disk.

### 5.2 Keyword search (`src/bible_search.rs`, new, core)

**Query parsing** (`SearchQuery::parse(&str)`): split into terms and quoted phrases; normalise each word (Unicode lowercase; fold `’`/`'` and remove them; drop every character that is not a letter or digit). Empty after normalising → error "Type at least one word to search for.". More than 12 terms → only the first 12 count.

**Index** (`SearchIndex`, one per translation, built from `for_each_verse`):

- `verses: Vec<VerseId>` (ordinal = Bible order), `texts: Vec<Box<str>>` (display text), `norm: Vec<Box<[u32]>>` (each verse as word ids, for phrase and proximity checks).
- `words: Vec<(Box<str>, Vec<u32>)>` sorted by word; each posting list is sorted verse ordinals.
- Lookup: a term of 3+ letters takes the union of postings for every word that starts with it (binary search for the prefix range); a shorter term matches its word exactly. All terms are intersected (AND).
- Quoted phrases: their words are intersected like terms; candidates are confirmed by scanning `norm` for the consecutive words. Phrases are filters.
- Scope filter: Old Testament = books 0–38, New = 39–65, One book = that book; applied as an ordinal range before intersection.
- **Ranking** (`rank(hits, query) -> Vec<u32>`, pure): each hit gets a tier: **1** the whole query's words appear consecutively in order (prefix rule per word, punctuation ignored), **2** all words fall inside a window of `2 × terms + 2` words, **3** the rest. Sort by (tier, ordinal). "Bible order" sorts by ordinal only. The first 200 are returned with the total.
- Example (bundled KJV, `be still and know`): three hits — Genesis 41:21 (tier 3), 1 Kings 22:3 (tier 3), Psalm 46:10 (tier 1). Best match lists Psalm 46:10 first.
- **Building.** Lazily, the first time a translation is searched, in one `tokio::task::spawn_blocking` task holding a cancel flag. Pre-warm: once the room output has committed its first frame (or 10 s after start-up when no room window is open), one `spawn_blocking` task builds the index for the Bible box translation; it is cancelled (flag checked inside `for_each_verse`) if a search starts for another translation, and restarted lazily later. At most one build runs at a time. Kept in memory for the app's lifetime. Target on a 2015 dual-core laptop: build ≤ 2 s, ≤ 15 MB per translation, query + rank ≤ 50 ms.
- While building, a search returns `Pending`; the dialog shows "Getting KJV ready to search…" and repeats the query when the build finishes (`Update::SearchReady(code)`).

For a `Provider` translation, the query is forwarded with the same `SearchQuery` and limit, and the provider's order is kept; network failures show "Searching NIV needs the internet. Check the connection, or search in KJV." For `None`: "Search isn't available for NLT. Try another translation."

**Snippet**: `snippet(text, query) -> String` returns up to 160 characters, starting at a word boundary at most 40 characters before the first match, with "…" where cut.

### 5.3 Actions (through `App::do_action`, as Plan 3 requires)

| Action | Body | Result |
| --- | --- | --- |
| `bible/search` | `{query, translation, scope: "all"\|"ot"\|"nt"\|{"book":u8}, order: "best"\|"bible"}` | `{state:"ready", total, hits:[{id, reference, snippet}]}` or `{state:"pending"}` |
| `bible/chapter` | `{translation, book, chapter}` | `{verses:[VerseText]}` |
| `bible/lookup` (extended) | adds optional `second: String`, `second_required: bool` | adds `second: {translation, verses, notice}` or, when the second fails and `second_required` is false, `second: null, second_error: String`; a main failure fails the whole action; with `second_required` a second failure fails it too ("Couldn't get NIV: …") |
| `settings/bible` | `{second_translation: Option<String>, layout}` | `{}`; rejects unknown names ("Unknown translation."), a translation that is not ready (its reason), and second == current main ("Pick a different second translation.") |
| `settings/translation` (extended) | unchanged body | when the new main equals `bible.second_translation`, also sets it to `None`; result gains `second_turned_off: bool` |
| `displays/room_size` | `{w, h}` | `{}`; sent by the UI when the room window opens; ignored when unchanged |

None of these touch `Live`. None are on the phone allowlist (`server.rs` keeps rejecting them). The Bible box sends `bible/lookup` with `second_required: false` (church default may lapse); Item edit sends `second_required: true` (explicit choice). Both then use `service/add` / `service/update`.

**Settings Save keeps live fields** (extends Plan 3 Task 20): before sending, Save copies `translation`, `displays` **and `bible`** from the latest snapshot into the edited clone. The Settings Bible tab edits `bible` only through `settings/bible`, applied at once, so a Settings save never undoes a second-translation change made from the top bar or by another operator.

### 5.4 Grouping verses into slides (`src/slides.rs`, pure)

```rust
pub fn plan_scripture(input: &ScriptureInput, m: &mut dyn Measure) -> PlannedScripture;
pub struct PlannedScripture { pub slides: Vec<Slide>, pub notes: Vec<String>, pub auto_gaps: BTreeSet<VerseId> }
pub trait Measure { fn fits(&mut self, main: &str, second: Option<&ParallelText>) -> Fit; }
pub enum Fit { Comfortable, Shrunk, TooLong }
```

`ScriptureInput` holds the verses of both translations, `per_slide`, `layout`, `breaks`, translation codes, notices, the resolved theme and the canvas.

- `Comfortable`: fits at a font size ≥ `READABLE = max(0.75 × or_default(theme.scripture_size, DEFAULT_SCRIPTURE_SIZE), 40)` px at 1080p scale (the resolved size, so `scripture_size = 0` gives 48).
- `Shrunk`: fits only between `MIN_PX` (26) and `READABLE`.
- `TooLong`: does not fit even at `MIN_PX`.

**Verse sequence.** Iterate the **union** of `VerseId`s from both translations in order. Each entry has main text, second text, or both. Verses without an id (unparseable old labels) keep their stored order after the previous id'd verse.

**Algorithm:**

1. Start a new group when the verse is in `breaks.before` (and the group is not empty).
2. Else, if the verse is in `breaks.join`, add it to the group.
3. Else apply the automatic rule: with `per_slide = N > 0`, start a new group once the group has N verses; with Automatic, add the verse if the group plus it is `Comfortable`, otherwise start a new group.
4. A finished group that is `TooLong` is split inside the text with the existing fallback order (sentence end → clause `; : ,` → word), into the fewest parts that are each `Comfortable` (or `Shrunk` if a single word run cannot be). In parallel mode both translations split into the **same number of parts**, each at its own boundaries nearest to equal shares. Parts after the first are labelled `(cont.)`.
5. Each slide gets `source = {first, last, part}`, its label from `join_refs` over the union verses on that slide (exact discontiguous references, v1 §8.4), the reference line `"<refs> (KJV)"` or `"<refs> (KJV · NIV)"`, and the footer notices.
6. `auto_gaps` = the gaps where a baseline run (same input, `join` ignored, `before` applied) starts a new slide. The breaks editor uses it.

**Gap state** (`pub fn gap_state(auto_break: bool, before: bool, join: bool) -> GapState`, pure, table-tested):

| auto | before | join | GapState | Saved as |
| --- | --- | --- | --- | --- |
| false | false | false | None | — |
| true | false | false | Auto | — |
| false | true | false | Added | before |
| true | true | false | Added | before (keeps the break if the planner moves) |
| false | false | true | None (stale) | dropped on save |
| true | false | true | Kept | join |
| false | true | true | Added | before (join removed) |
| true | true | true | Added | before (join removed) |

Clicks act on the displayed state (§3.3 step 3).

**Missing verses.** A verse present in only one translation (e.g. Acts 8:37 in KJV, not NIV) is still a verse of the item: it gets its own row in the breaks editor and its own gap, its text appears in the column that has it and the other column shows nothing for it, and the item's operator note (under the slides grid) reads "NIV doesn't include Acts 8:37." Labels list every union verse on the slide.

**Changing translations of an item with breaks.** A `before` mark on a verse that no longer exists in the union moves to the **nearest existing verse boundary** (v1 §8.4); when two are equally near, the later one. A `join` mark on a missing verse is dropped.

### 5.5 Planning off the command queue (`src/scripture_plan.rs`, new, core)

`build_item` runs synchronously in the command path (`DataPlan::build` in `App::command`, state building, and the phone snapshot in `server.rs`). It must never shape text.

```rust
pub struct PlanKey(u64);   // hash of verses (both), breaks, per_slide, layout, codes, notices,
                           // resolved theme (all fields), canvas
pub struct PlanCache { map: RwLock<HashMap<Uuid, (PlanKey, Arc<PlannedScripture>)>> }
pub struct Planner { tx: SyncSender<PlanJob>, … }   // one thread "scripture-planner"
```

- **Planner thread.** One dedicated thread with its **own** `Fonts` (not shared with the render worker, so there is no lock contention with room rendering) and a `LayoutMeasure` that runs the real `layout()`; 4,096-entry LRU keyed by `(text hash, second, theme, canvas)`. Jobs are coalesced per item (latest wins). After every Data update, the app submits jobs for scripture items whose `PlanKey` changed; it also submits all scripture items when the planning canvas or the resolved theme changes. Completion writes `PlanCache` and emits `Update::PlansReady(Vec<Uuid>)`.
- **`build_item` only reads.** It computes the `PlanKey` (hashing ~100 KB for a 400-verse parallel item: well under 1 ms) and uses the cached plan when the key matches. Otherwise it plans with `CharMeasure` (the deterministic character-budget estimate, microseconds) and marks the item `fitting: true` in state; the slides grid header shows a small chip "Fitting slides…". The command path never calls `LayoutMeasure`.
- **Canvas.** Plans use `displays.room_size` (last size the room window opened at), or 1920×1080 when never opened. Unplugging the projector or closing the room window changes nothing. When the room window opens at a different size, the UI sends `displays/room_size`, which re-plans all scripture items. A 4:3 projector therefore plans at 1024×768 once it has been opened once.
- **Before FEAT-1** the resolved theme is `settings.theme`. After FEAT-1 it is FEAT-1's resolver (item > service > church default), which falls back to the church default for a deleted theme id; the `PlanKey` includes the resolved theme, so the breaks re-plan automatically.

### 5.6 Live safety, navigation and the Bible box

- Search, chapter view, the breaks editor and Item edit never change the room. Only Take Live / thumbnail clicks / Next / Prev do, through existing commands with ids and base versions (v1 §5.4). `Command::Show { item_id, slide }` keeps its literal meaning: show that index.
- **Live item changed notice** (v1 §7.2; built here for every item kind). After each Data update and each `PlansReady`, the app compares the live snapshot's slides with `DataPlan::build(live item)` (cheap: a cache read and a `Vec<Slide>` comparison). When they differ it sets `live.notice = "Live item changed — Take Live to update."`; a Show of that item clears it. The live snapshot itself never changes.
- **Re-take mapping happens in the operator UI.** When the operator presses Take Live or `Enter` and the selected item is the live item, `src/ui/operator/` computes the target with `slides::index_for_source(old: Option<&SlideSource>, new: &[Slide]) -> usize` (pure): the new slide whose source range contains `old.first` with the same `part` if present, else part 0 of it; with no source (non-scripture), the same index clamped; fallback 0. It then sends an ordinary Show with that index. Thumbnail clicks send the clicked index unchanged. Plan 3 Task 11's "Enter on an armed item sends slide 0" still holds for items that are not live.
- **Selection and arm across re-plans.** When an item's slides change (edit, plan finished, canvas change), the UI maps the selected thumbnail and the armed slide with `index_for_source`.
- Clear matrix: unchanged. A parallel slide is an ordinary slide-layer frame; Clear Slide removes both columns. FEAT-1 overlays sit above it as on any slide.
- A failed main lookup leaves the service unchanged; the Bible box keeps its text and shows the core error.

**Bible box input** (`pub fn classify_bible_input(&str) -> BibleInput`, pure, in `src/bible.rs`):

```rust
pub enum BibleInput { Reference(Reference), BookOnly { book: u8 }, UnknownWithDigits, Words }
```

1. `parse_reference` succeeds → `Reference` → look it up (unchanged).
2. Else the trimmed text is a book name or unique prefix (`find_book` on the whole text: "Job", "Mark", "judge", "numbers") → `BookOnly` → the existing error "Add a chapter, like Job 1 or Job 1:1." in the Bible box hint row, with a button **Search for "job" instead**.
3. Else the text contains any digit ("Hezekiah 3:1", "40 days", "seventy times 7") → `UnknownWithDigits` → the existing error "I don't recognize the book…" with a button **Search for these words**.
4. Else → `Words` → open Find a verse with the text. If the search finds nothing and the text is one word, the empty list says "No verses in KJV have that word. If it's a book name, check the spelling and add a chapter, like John 3." ("Phillipians" lands here).

`Ctrl+Enter` skips classification and always searches. The hint row sits under the Bible box (one line: message + one button, focusable with `Tab`), and disappears when the text changes.

### 5.7 Edge cases

- Search query that is only short words (`a of`): allowed, exact matches, capped at 200 shown.
- Typing while the index builds: "pending"; typing more replaces the waiting query.
- The 400-verse lookup limit applies to Add from the context view as well.
- `per_slide = 1` with `join` marks: joins still merge verses.
- Second translation not set up: Bible box adds the main only with the warning toast (§3.2); Item edit reverts the drop-down with the error.
- Main set equal to second: second turned off (§3.2).
- Room window opens at a new size while an item is live: live output unchanged; the notice appears if the live item's plan changed.
- Item references a deleted theme (FEAT-1): church default, re-planned.
- Old scripture items without `id`s on a verse: they plan by order, breaks can't be added on that verse (gap disabled, tooltip "This verse came from an older version; look the passage up again to choose breaks here.").

## 6. Rendering (`src/render`)

- `layout.rs`: `layout()` checks `slide.second`. When `Some`, it calls new `layout_parallel()`:
  - **Side by side:** body box split into two columns with a 4 % canvas-width gutter and a 2 px divider at 25 % opacity of the theme text colour. Each column starts with a heading (translation code, `REFERENCE_PX × 0.8`, weight 600, theme colour at 70 % alpha).
  - **Stacked:** main block on top, divider, second block below; headings at the left.
  - Text is **left-aligned** in both layouts, as single-translation scripture is today (`layout.rs` scripture branch).
  - One shared font size: `fit_steps` shrinks until **both** blocks fit; the larger block decides. Lower-third theme position (`position = "lower"`): stacked is used whatever the item says (the UI disables Side by side there, §3.2).
  - Reference line unchanged (bottom right). The attribution footer sits under it at `REFERENCE_PX × 0.7`; space for both is reserved first (v1: text may never overlap the reference/attribution area). When the footer is too wide it shrinks to `MIN_PX × 0.6` then wraps to two lines before the body loses any size.
- `TextLayout` gains `second_body: Option<Buffer>`, `second_box: Option<Rect>`, `headings: Option<(Buffer, Buffer)>`, `footer_box: Option<Rect>`; `raster.rs` draws them with the body's shadow rules.
- `measure.rs` (new): `LayoutMeasure` (§5.5) and `pub fn fit_px(fonts, slide, theme, canvas) -> f32`. Used only by the planner thread and the breaks dialog worker.
- Thumbnails for the breaks editor and the Settings preview use the existing `Renderer` (cache + worker) at 384 px wide and the planning canvas aspect.

## 7. UI (extends Plan 3)

**Strings.** Rust-side strings live in `src/strings/bible.rs` (`pub const`s and small `fn`s for strings with values), in the shared `src/strings/` tree that FEAT-4 also uses (`src/strings/mod.rs` is created by whichever FEAT lands first). Core errors and UI messages both come from there. Fixed labels in new `.slint` files use `@tr("…")` (as FEAT-2/FEAT-6 do); text with values is passed in as properties from `strings::bible`. Wording follows `web/operator.js`: short, plain, second person.

| Plan-3 file | Change |
| --- | --- |
| `ui/operator/topbar.slint`, `src/ui/operator/bible.rs` | `+ NIV` second chip with X, `+` menu "Also show…" (set-up translations only), hint row with one action button, `Ctrl+Enter` = search; submit uses `classify_bible_input`; main-only fallback toast; "Second translation turned off" toast |
| `src/ui/operator/` slides grid header (Task 7) | **Slide breaks** button for scripture items; "Fitting slides…" chip |
| `src/ui/operator/` live column / keys (Tasks 8, 11) | Live-item-changed notice shown from `live.notice`; re-take uses `index_for_source`; selection/arm mapped across re-plans; sends `displays/room_size` when the room window opens |
| `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs` | Translation, Second translation, Layout controls (set-up only; lower-third note); **Choose slide breaks…** |
| `ui/dialogs/add.slint`, `src/ui/dialogs/add.rs` | Bible tab: **Search by words…** link; Enter classifies like the Bible box |
| Settings Bible tab (Task 20) | Second translation drop-down, Layout segmented (lower-third note), preview thumbnail, per-translation "Search: Ready / Needs internet / Not available" chip; applied through `settings/bible`; Save copies `bible` from the latest snapshot |
| Settings Help tab | The search rules from §3.1 and the breaks editor keys |
| `src/ui/dialogs/mod.rs` | `DialogKind::BibleSearch { query: String }`, `DialogKind::ScriptureBreaks(Uuid)`; both are **not** refreshed by `dialog_refresh` (they hold user input) |

**New files:**

- `ui/dialogs/bible-search.slint` — `BibleSearchDialog`: `in-out property <string> query`, `in property <[SearchHitRow]> hits` (`{reference, snippet}`), `in property <string> status`, `in property <[ContextVerse]> context` (`{number, text, hit: bool, selected: bool}`), `in property <string> add-label`, `in property <[string]> translations`, `in property <[bool]> translation-ready`, `in-out property <int> translation-index`, `in-out property <int> scope-index`, `in-out property <bool> bible-order`, `in property <[string]> books`, `in-out property <int> book-index`; callbacks `query-edited()`, `query-submit()`, `pick-hit(int)`, `pick-verse(int, bool /*extend*/)`, `extend(int /*-1|+1*/)`, `add()`.
- `src/ui/dialogs/bible_search.rs` — debounce (250 ms `slint::Timer`; `query-submit` fires it at once), stale-result guard (each request carries a sequence number; older replies are dropped), range selection model (`Selection { anchor, head }`, pure, clamped to the chapter), add flow reusing the Bible box insert/arm/toast code (factored into `add_scripture_item(translation, second: Option<String>, …)`).
- `ui/dialogs/scripture-breaks.slint` — `ScriptureBreaksDialog`: `in property <[GapRow]> rows` (`{verse-number, preview, second-only: bool, gap: string /*"none"|"auto"|"added"|"kept"*/, enabled: bool}`), `in property <[Thumb]> slides` (`{image, label, warn}`), `in property <string> note`, `in property <bool> busy`, `in-out property <int> per-slide`; callbacks `toggle-gap(int)`, `reset()`, `save()`.
- `src/ui/dialogs/scripture_breaks.rs` — works on a clone of the item; `fn click(state: GapState) -> Mark` (pure); re-plans with `plan_scripture` + its own `LayoutMeasure` in `spawn_blocking` after each click (stale results dropped by sequence number) and renders thumbnails through `Renderer`; Save drops stale `join` marks then sends `service/update`.

All colours and fonts come from the `Theme` global. Every control has an accessible name so the testing backend can find it.

## 8. Errors

All messages live in `src/strings/bible.rs`.

| Situation | Message | Effect |
| --- | --- | --- |
| Empty search | "Type at least one word to search for." | Nothing searched |
| No hits | "No verses in KJV have all of those words." | Empty list, context cleared |
| No hits, one word | "No verses in KJV have that word. If it's a book name, check the spelling and add a chapter, like John 3." | Empty list |
| Too many hits | "Showing the first 200 of 1,240 verses — add more words to narrow it down." | First 200 shown |
| Index building | "Getting KJV ready to search…" | Retries automatically |
| Provider search offline | "Searching NIV needs the internet. Check the connection, or search in KJV." | Nothing searched |
| Translation without search | "Search isn't available for NLT. Try another translation." | Nothing searched |
| Book name alone | "Add a chapter, like Job 1 or Job 1:1." + **Search for "job" instead** | Hint row |
| Digits, unknown book | existing "I don't recognize the book…" + **Search for these words** | Hint row |
| Second = main (item) | "Pick a different second translation." | Item unchanged |
| Main changed to second | "Second translation turned off (same as main)." | Toast; second off |
| Church second not set up | "NIV isn't set up, so only KJV was added. Turn off the second translation with the X." | Main-only item added, warning toast |
| Item-edit second lookup fails | "Couldn't get NIV: `<core message>`" | Drop-down reverts |
| Missing verse in one translation | "NIV doesn't include Acts 8:37." | Operator note on the item |
| Verse too long | "Too long — will split inside a verse" | Warning chip on the thumbnail |
| Join made a slide too long | "Keeping these together makes a slide too long, so it will split inside a verse." | Inline note in the breaks dialog |
| Item removed while editing breaks | "That item is gone." (core, Plan 3) | Toast; dialog stays open, Save still active |
| Live item edited | "Live item changed — Take Live to update." | Live column notice |
| First start after upgrade | "Bible slides are now fitted to the screen, so some passages may break in different places." | One-time toast |

### 8.1 Note on highlighting

Slint 1.18 `Text` cannot style part of a string. Results show the trimmed snippet and the context view highlights the whole hit verse. Word highlighting can come later without changing the core API (`hits` can gain match ranges).

## 9. Testing

**Unit tests (core):**

- `bible_search.rs`: `query_parses_terms_and_quoted_phrases`, `query_normalises_case_and_apostrophes`, `empty_query_is_an_error`, `prefix_matches_loveth_and_loved`, `short_terms_match_exactly`, `all_terms_must_match`, `phrase_requires_consecutive_words`, `scope_old_testament_excludes_john`, `scope_one_book`, `bible_order_option_sorts_by_ordinal`, `rank_tiers_phrase_then_proximity_then_rest`, `be_still_and_know_ranks_psalm_46_10_first` (full bundled KJV; also asserts exactly 3 hits), `caps_at_200_and_reports_total`, `snippet_trims_around_first_match`, `prewarm_cancels_when_other_translation_searched`, `index_build_under_two_seconds_release` (`#[ignore]`, run manually on the old laptop).
- `bible.rs`: `parse_verse_label_round_trips_display_names` (all 66 books, "Psalm" singular), `lookup_fills_verse_ids`, `classify_reference`, `classify_book_only_job_mark_judge_numbers`, `classify_digits_unknown_book`, `classify_words`, `kjv_reports_all_verses_and_empty_notice`.
- `model.rs` / `store.rs`: `old_scripture_item_loads_with_defaults`, `fill_verse_ids_from_labels`, `restore_old_backup_fills_verse_ids`, `settings_without_bible_section_loads`, `parallel_layout_serializes_snake_case`, `verse_breaks_is_empty_skips_serializing`, `upgrade_toast_shown_once`.
- `slides.rs`: `auto_groups_by_measure_not_chars`, `manual_break_before_starts_new_slide`, `join_overrides_auto_break`, `join_too_long_splits_inside_verse_with_cont_label`, `per_slide_n_with_manual_break`, `before_on_first_verse_is_ignored`, `parallel_pairs_verses_by_id`, `parallel_missing_verse_leaves_column_empty_and_notes_it`, `label_includes_verse_only_in_second_translation`, `parallel_long_verse_splits_both_into_same_part_count`, `reference_line_names_both_translations`, `discontiguous_labels_stay_exact`, `slide_source_set_for_every_slide`, `breaks_move_to_nearest_existing_boundary_on_retranslate`, `gap_state_table` (all 8 rows), `auto_gaps_ignore_join_marks`, `readable_uses_resolved_scripture_size`, `index_for_source_same_first_verse`, `index_for_source_falls_back_to_zero`.
- `scripture_plan.rs`: `build_item_uses_cached_plan_when_key_matches`, `build_item_falls_back_to_char_measure_and_marks_fitting`, `plan_key_changes_with_theme_breaks_canvas_second`, `deleted_item_theme_replans_with_church_default` (after FEAT-1; before it, `theme_change_replans`), `unplugged_room_keeps_last_known_canvas`, `room_size_change_replans_all_scripture`, `planner_coalesces_jobs_per_item`.
- `app.rs`: `take_live_never_measures` (a `Measure` that panics when a thread-local "in command" flag is set by `App::command`; Show, Next, state building and the phone snapshot run on a 400-verse parallel item), `next_on_400_verse_parallel_item_is_fast` (command handling under 20 ms in release, `#[ignore]` in debug), `bible_search_action_never_touches_live`, `bible_lookup_second_optional_returns_second_error`, `bible_lookup_second_required_fails_whole`, `service_update_rejects_same_second_translation`, `service_update_drops_unknown_break_verses`, `settings_bible_rejects_unknown_translation`, `settings_bible_rejects_same_as_main`, `picking_main_equal_to_second_turns_second_off`, `live_item_changed_sets_notice`, `show_clears_live_item_changed_notice`, `remote_cannot_call_bible_search` (server allowlist).
- `live.rs`: `show_uses_the_given_index`, `editing_breaks_of_live_item_keeps_committed_frame`.
- `render`: `parallel_columns_share_one_font_size`, `stacked_forced_in_lower_third`, `parallel_text_left_aligned_in_both_layouts`, `parallel_slide_shows_both_attributions`, `attribution_shrinks_before_body`, `parallel_render_key_differs_from_single`, `layout_measure_caches_repeat_calls`.

**UI tests (Slint testing backend, `Ops::inline`, Plan 3 helpers):**

- Search: `bible_box_words_open_search_dialog_without_adding`, `bible_box_unknown_book_with_digits_keeps_error`, `single_word_book_name_offers_search`, `words_with_digits_offer_search`, `ctrl_enter_always_searches`, `search_typing_debounces_and_shows_hits`, `enter_in_search_box_focuses_first_result`, `search_down_arrow_enters_results`, `shift_click_extends_range_and_add_label_updates`, `shift_down_stops_at_chapter_end`, `search_enter_adds_after_live_and_arms`, `search_add_uses_dialog_translation_and_church_second`, `second_enter_takes_search_result_live`, `f1_in_search_dialog_clears_and_dialog_stays`, `stale_search_reply_is_ignored`, `unready_translation_greyed_in_search_dropdown`.
- Two translations: `second_chip_x_turns_second_off_and_saves`, `plus_menu_lists_only_ready_translations`, `bible_box_adds_parallel_item`, `unready_second_adds_main_only_with_warning`, `alt_1_equal_to_second_shows_turned_off_toast`, `save_settings_keeps_second_translation_changed_meanwhile`, `side_by_side_disabled_for_lower_third_theme`.
- Item edit: `item_edit_change_translation_relooks_up`, `item_edit_failed_lookup_reverts_dropdown`, `item_edit_layout_hidden_without_second`.
- Breaks: `breaks_dialog_click_gap_cycles_states`, `breaks_dialog_space_toggles_focused_gap`, `breaks_dialog_reset_then_cancel_keeps_original`, `breaks_dialog_join_too_long_shows_note`, `breaks_dialog_save_drops_stale_joins`, `breaks_dialog_save_on_live_item_shows_take_live_note`, `breaks_dialog_save_on_removed_item_shows_item_gone`.
- Live: `retake_live_scripture_lands_on_same_first_verse`, `thumbnail_click_sends_literal_index`, `canvas_change_maps_selection_by_source`, `fitting_chip_shows_until_plan_ready`.

**Manual checks (church laptop and one old laptop, Windows; one macOS or Linux machine):**

1. Search "be still and know" → Psalm 46:10 first in Best match, third in Bible order; results under a second after the first search.
2. Two translations side by side on the projector from the back row: both columns readable for John 3:16–18 at the default theme. Repeat stacked and with a lower-third theme.
3. Romans 8:28–39 in parallel mode: no slide text below readable size without a warning; no overlap with the reference or attribution lines.
4. Add a manual break while the passage is live: room unchanged, notice shows; Take Live lands on the same verse.
5. Old laptop: first search time; memory in Task Manager before/after indexing; press Next rapidly through a freshly added 150-verse parallel passage — no stutter, Clear (F1) immediate.
6. Unplug the projector mid-service and plug it back: slide counts unchanged.

## 10. Dependencies on other FEAT specs

- **Plan 3 (required first):** Theme, Ops, dialogs, Live column, Item edit (Task 18), Settings (Task 20, extended "Save keeps live fields"), keys (Task 11).
- **FEAT-1 (optional):** before FEAT-1, measurement and rendering use `settings.theme`; after it, FEAT-1's resolver (item > service > church default, deleted id → church default). If FEAT-1 adds a minimum readable size to `Theme`, `READABLE` uses it. FEAT-1 rich text runs replace `SlideContent::Text.text`; `ParallelText.text` follows. Overlays render above parallel slides with no special case.
- **FEAT-3 (Stage):** no work. The stage receives the main translation in `SlideLayer::Text.text` (matches FEAT-3 "two-translation slides arrive as normal text"). `SlideLayer::Text.second` is there if a later stage cell wants it.
- **FEAT-4:** may reuse the live-item-changed notice and `index_for_source`-style mapping for live song editing.
- **FEAT-6 (Service planning):** templates and "copy last week" copy scripture items with `second`, `layout` and `breaks` intact. Printed orders list `John 3:16–18 (KJV · NIV)` and both `copyright_notice()`s. Practice mode needs nothing from FEAT-5.
- **FEAT-7 (Livestream, CCLI, PCO):** Looks decide whether the livestream shows parallel text; a lower-third output should use the main translation only. Scripture is not in the CCLI song usage report. PCO import uses the same `TranslationProvider`.
- **Bible translation sources (other session):** implement `TranslationProvider` (§5.1), fill `VerseText.id`, report `SearchAccess` honestly (partial caches never `AllVerses`), and supply `display_name()`/`copyright_notice()`. Until then only KJV searches; other translations show the "not set up" messages.

## 11. Decisions for Jake to review

1. **Words in the Bible box open search.** Default: typing words (no recognised reference, no digits, not a book name) and pressing Enter opens Find a verse with results; a book name alone or text with digits keeps its error but offers a "Search for…" button; `Ctrl+Enter` always searches. Alternative: keep the Bible box for references only and use a separate search button.
2. **Search matching.** Default: every word must appear; words match their beginnings (love → loved, loveth); quotes for exact phrases. Alternative: any word may match, ranked by how many match.
3. **Result order.** Default: best match first (exact words in order, then words close together, then the rest in Bible order), with a "Bible order" toggle. Alternative: Bible order only (Genesis to Revelation).
4. **Result limit.** Default: show the first 200 and say how many there are. Alternative: show all, however many.
5. **No search on the phone.** Default: the phone remote stays as it is. Alternative: add a read-only search to the phone page (that would grow the allowlist).
6. **Search index kept in memory only.** Default: built when first needed (and quietly after the room is up, for your usual translation), never saved to disk. Alternative: save the index to disk for an instant first search.
7. **Second translation off by default.** Default: one translation unless a church turns on a second in Settings, the top bar or an item. Alternative: ask during first-run setup.
8. **Top-bar X and + change the church default.** Default: turning the second translation on or off in the Bible box is saved for everyone until changed back (like the main translation chip). Alternative: they apply to the next passage only and reset afterwards.
9. **Second translation that isn't set up.** Default: the Bible box adds the main translation alone and shows a warning toast. Alternative: refuse to add anything until the second is turned off.
10. **Main translation picked equal to the second.** Default: the second is turned off, with a toast. Alternative: swap them (the old main becomes the second).
11. **Side by side is the default layout.** Default: two columns, same text size, text left-aligned. Alternative: one above the other by default (better for very long passages and narrow screens).
12. **Lower-third looks always stack.** Default: with the lower-third text position, two translations show one above the other and "Side by side" is greyed out. Alternative: allow side by side there too.
13. **Stage and phone show only the main translation.** Alternative: stage shows both (main, then second).
14. **Both copyright notices on every parallel slide.** Default: a small line under the reference with both notices. Alternative: notices on the first slide of the passage only (only if the licences allow it).
15. **Missing verses.** Default: show the verse in the translation that has it, leave the other column empty for it, list it in the label, and tell the operator. Alternative: hide the verse from both sides.
16. **Breaks stay on verse boundaries.** Default: you can add a break between verses or keep two verses together; only the automatic splitter breaks inside a very long verse. Alternative: let volunteers break anywhere inside a verse.
17. **Break marks sit on top of automatic breaks.** Default: "New slide here" and "Kept together" adjust the automatic breaks; a "Kept together" mark is dropped when the automatic break under it goes away. Alternative: the first click turns automatic off for that item and every break becomes manual.
18. **"Readable size" for automatic breaks.** Default: a new slide starts before text would shrink below 75 % of the scripture text size (never below 40 px on a 1080p screen). Alternative: fill each slide as full as possible down to the smallest size (fewer slides, smaller text).
19. **No automatic break at chapter boundaries.** Default: John 1:51–2:2 can share a slide. Alternative: always start a new slide at a new chapter.
20. **Slides are fitted to the last-known room screen size.** Default: the size the room window last opened at (1920×1080 until then); unplugging the projector changes nothing. Alternative: fit to whatever screen is attached right now.
21. **Existing passages are re-fitted after the upgrade.** Default: old scripture items use the new measured breaks, with a one-time toast. Alternative: keep the old character estimate for items added before the upgrade.
22. **"Live item changed" notice for every item kind.** Default: any edit to the live item (songs too) shows "Live item changed — Take Live to update." Alternative: scripture only.
23. **"Choose slide breaks…" in Item edit saves first.** Default: the button saves your Item edit changes, then opens the breaks editor. Alternative: open the breaks editor without saving and save both together (more complex).
24. **Reset to automatic asks no question.** Default: it resets in the dialog; Cancel brings everything back. Alternative: a confirm box.

## Review notes

- FEAT-3 dependency: resolved by making FEAT-5 match FEAT-3 (stage shows main only) rather than editing FEAT-3, so the two specs agree without a cross-spec change.
- Strings convention: the sibling specs still differ (`src/strings/<area>.rs` in FEAT-4, `pub mod text` in FEAT-7, `stage/strings.rs` in FEAT-3, `@tr` in FEAT-2/6). FEAT-5 follows FEAT-4's shared `src/strings/` tree plus `@tr` for fixed `.slint` labels; the roadmap owner should pick one tree for all FEATs before implementation.
- Breaks model: kept two marks over automatic breaks (now fully defined by `gap_state`) instead of the fully manual model; the alternative is Decision 17.
