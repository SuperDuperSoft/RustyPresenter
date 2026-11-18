# FEAT-5 — Bible Extras: Keyword Search, Two Translations, Slide Breaks — Design Spec

**Date:** November 1, 2026 (revision 3, cross-spec alignment; see the note at the end)
**Status:** Draft for review (written while Jake was away; see "Decisions for Jake to review")
**Roadmap:** `PROJECT_PLAN.md` §10b, FEAT-5. Depends on Plan 3 (native operator) and on the Bible session's interface (branch `bible-esv`, §5.1). Works before or after FEAT-1 (§10).
**Controls:** v1 spec §5.4 (command ids), §5.5 (responsiveness), §6 (layers, clear matrix), §7 (navigation), §8 (Bible). This spec adds to §8; it does not change any v1 rule. The v1 §7.2 "live item changed" handling is FEAT-4's reconcile and banner; FEAT-5 supplies the scripture position mapper it uses (§5.6).

## 1. Purpose and who it's for

Volunteers at a typical small church put scripture on screen in three situations the v1 Bible box does not cover well:

1. **"I know the words, not the reference."** The pastor says "be still and know that I am God". The volunteer needs to find Psalm 46:10 in seconds, without leaving the app. (Search matches the words as typed and their longer forms: `know` finds knoweth, but `being` does not find be.) Search covers the translations stored on this computer (KJV and bundled or imported texts, §5.2); the online providers' terms forbid downloading or indexing their whole text.
2. **Two translations at once.** A pastor who preaches from the NIV but wants the KJV wording beside it, or a study series comparing wordings. ProPresenter and EasyWorship both offer this. The translation list is data-driven: it is whatever the Bible session reports in `bible_sources` (§5.1). A bilingual congregation can use this once a non-English source exists (versification mapping is the Bible session's job).
3. **"That slide break is in the wrong place."** Automatic splitting is good but not perfect: a verse that belongs with the next one ends up alone, or a long passage needs a break where the pastor pauses. Today the only control is "Verses per slide".

The people using this are non-technical volunteers on old laptops. Every flow below works with the keyboard alone, never touches the room output until the volunteer shows something, and explains any problem in plain words.

The app is free for every church. Which translations must work before a release is Jake's call (the earlier "all six translations" gate is with him); this spec works with whatever list `bible_sources` reports.

## 2. Scope

**In:**

- Keyword search across a whole **local** translation (status `offline`), with an Old/New Testament and single-book filter, best-match ranking, a context view of the chapter, and adding a hit (or a range around it) to the service. Optional: ESV online search through ESV's own `/search` endpoint, only if the Bible session exposes it (§5.2).
- Searching from the Bible box: typing words instead of a reference opens the search; `Ctrl+Enter` always searches.
- A second translation on a scripture item, shown **side by side** (default) or **one above the other**, with both copyright notices.
- A church default for the second translation (off by default) and a per-item choice.
- Choosing slide breaks per scripture item: force a new slide before a verse, keep two verses together, reset to automatic. A visual editor with live slide thumbnails.
- Measured fitting: automatic breaks are decided by the real slide layout (src/render), as v1 §8.3 requires, instead of today's 330-character estimate. Measuring runs on a planner worker, never in the command queue (§5.5).
- Stable verse identity (`VerseId`), derived from each verse's reference label by `scripture::verse_id_of`, carried on generated slides (`SlideSource`), so breaks survive translation changes and FEAT-4's "Update screen" lands on the right slide (v1 §8.4).
- The storage rule for copyrighted text: verse text of `online` or keyed translations never goes into `data.json`; it lives in a bounded per-translation cache (§5.1a).
- Settings → Bible: per-translation status, "Get one" links and key fields (sent with the Bible session's `bible/key`), second translation and layout.

**Out:**

- Translation sources, licensing, downloads, API keys storage and everything under `src/bible*`. The Bible session owns those; FEAT-5 adds nothing under `src/bible*` and builds on its interface (§5.1).
- Search on the phone remote. FEAT-5 adds nothing to FEAT-2's allowlist table (FEAT-2 §5.11).
- Indexing, pre-warming or bulk-fetching any `online` translation.
- Manual breaks in the middle of a verse. Breaks stay on verse boundaries (v1 §8.4). Automatic mid-verse splitting of over-long verses stays.
- More than two translations on one slide. Two translations on the stage display (§3.2).
- Topic/concordance search (Strong's numbers, cross references), fuzzy spelling correction, stemming beyond prefix matching.
- Word highlighting inside search result snippets (the hit verse is highlighted as a row instead; §8.1).
- Lower-third/livestream formatting of scripture beyond the stacking rule (§3.2); FEAT-7 screen views decide what each output shows.
- The "live item changed" banner and its update command (FEAT-4).

## 3. User experience

### 3.1 Find a verse by its words (operator)

1. The volunteer presses `Ctrl+B`, types `be still and know` and presses `Enter`.
2. The Bible box classifies the text (`scripture::classify_bible_input`, §5.6). It is words, not a reference, so the **Find a verse** dialog opens with the words in its search box and results already showing. Nothing is added to the service and the room is untouched.
3. Results list (left): `Psalm 46:10` in bold, and below it the verse text, trimmed to about 160 characters around the first matched word with "…". The header says "3 verses found in KJV" (or "Showing the first 200 of 1,240 verses — add more words to narrow it down"). Results are ordered best match first (§5.2); a small toggle **Best match / Bible order** sits beside the header (remembered for the session).
4. Results update as the volunteer types (250 ms after the last key).
5. Keys in the search box: `Enter` runs any pending search at once (cancelling the debounce), waits for it, then moves focus to the first result. It never adds from the box. `↓` does the same without forcing a pending search (if a search is pending, focus moves when it arrives). With no results, `Enter` and `↓` do nothing.
6. In the list, `↑`/`↓` move through results. The context view (right) shows the whole chapter of the highlighted result, scrolled to the hit verse, which is selected and tinted with the accent colour.
7. Widening the selection: click a verse to select it alone, `Shift`+click (or `Shift+↑`/`Shift+↓` in the context view, `Tab` moves focus there) to extend the range. The range stays inside the shown chapter: `Shift+↓` on the last verse and `Shift+↑` on the first do nothing. The primary button always names what will be added: **Add Psalm 46:10–11**.
8. `Enter` (in the list or the context view) or the button adds that passage: in the **dialog's** translation, plus the church second translation when one is set and usable (§3.2). It is inserted after the live item (or at the end when nothing is live), selected and armed, dialog closed, toast "Psalm 46:10–11 added — press Enter to show it". A second `Enter` takes it live (v1 §8.1 flow, unchanged).
9. Filters in the dialog head: translation drop-down (defaults to the Bible box translation when it is searchable, otherwise the first local translation; lists every translation from `bible_sources`, with the ones that cannot be searched greyed and the tooltip "Search isn't available for NIV"), and a scope segmented control **Whole Bible / Old Testament / New Testament / One book**; "One book" shows a book drop-down. Scope is remembered for the session only.
10. `Esc` closes the dialog. F1–F6 still clear the room (Plan 3 Task 10 capture rule; keys per FEAT-1 §7's key table).

When the Bible box translation is not searchable (for example NIV, which is online), words typed there open the dialog on the first local translation (KJV) with the note "Search isn't available for NIV, so this searches KJV." Adding a hit still uses the dialog's translation; the volunteer can change the added item's translation in Item edit.

Other ways in: the Add dialog's Bible tab gets a **Search by words…** link that opens the same dialog with whatever is in its reference box; Enter in that tab's box classifies the text exactly as the Bible box does.

Search rules, written for the Help tab:

- Search works for the translations stored on this computer (KJV and any bundled or imported Bible). Online translations can't be searched, because their owners don't allow it (ESV may offer its own online search).
- Every word must appear in the verse (`love neighbour` finds verses with both).
- Words match their beginnings: `love` finds love, loved, loveth, lovest, loving. Words shorter than 3 letters must match exactly.
- Put words in quotes for an exact phrase: `"be still"`.
- Capitals and punctuation do not matter. `lord` finds LORD.
- Best matches come first: verses with your exact words in order, then verses where the words are close together, then the rest in Bible order. Switch to **Bible order** to see Genesis first.
- `Ctrl+Enter` in the Bible box always searches, even for a book name like "Job".

### 3.2 Show two translations

**Usable translations.** A translation is usable when its `bible_sources` status is `offline` or `online`. Others are greyed everywhere with the status wording from §5.1 ("Needs a key — Get one", "Not available").

**Church default.** Settings → Bible (FEAT-1 §7 Settings tab table) gets a **Second translation** drop-down: `None` (default), or any usable translation that differs from the main one. Under it, **Layout**: **Side by side** (default) / **One above the other**, with a live preview thumbnail of John 3:16–17 using the church theme. When the resolved theme uses the lower-third position, **Side by side** is disabled with the note "Lower-third themes always stack two translations." These controls apply at once through `settings/bible` (like the translation chips); the Settings dialog's Save does not send `bible` (FEAT-1 §5.3 merge rule, §5.3 below).

**In the Bible box.** When a second translation is set, a second chip appears after the translation chips: `+ NIV` with an X. Clicking the X turns the second translation off for the church (saved, like the main chip). When it is off, a small `+` button appears instead; it opens a menu "Also show…" listing usable translations other than the main one (others greyed with their status wording). `Enter` looks up both translations (`scripture/parallel`, §5.3), adds one item, arms it. Below 1500 px window width the second chip and `+` fold into the translation chip menu as an "Also show…" sub-menu (FEAT-1 §7.2 layout budget). The existing `Alt+digit` translation chip keys stay as they are.

- If the saved second translation is not usable (key removed, source gone) or its lookup fails, the chip shows in the `off` style with the status tooltip, and `Enter` adds the **main translation only**, with a warning toast: "NIV isn't available right now, so only KJV was added. Turn off the second translation with the X." The main translation is never replaced, so this is not a silent substitution (v1 §8.2).
- If the volunteer picks a main translation equal to the second (clicks the NIV chip or presses its `Alt+digit` while the second is NIV), the core turns the second off in the same `settings/translation` action and the UI shows the toast "Second translation turned off (same as main)."

**Per item.** Item edit (Plan 3 Task 18) is scrollable with sections Words, Theme, Automation, Stage and Bible (FEAT-1 §7.2). For a scripture item its **Bible** section holds:

- **Translation** drop-down (the main one; usable translations only).
- **Second translation** drop-down (`None` + usable translations other than the main).
- **Layout** segmented (Side by side / One above the other), shown only when a second translation is chosen; Side by side is disabled with the lower-third note when the item's resolved theme is lower-third.
- **Choose slide breaks…** (§3.3).

Changing a translation in Item edit looks the passage up at once (off the UI thread, `scripture::fetch`) and updates the dialog's preview. If the lookup fails, the drop-down returns to its previous value and an error toast shows the core's message. Nothing changes in the service until **Save**.

**On screen (room).** Side by side: two columns of equal width, each headed by its translation code (`KJV`, `NIV`) in the reference style; verse numbers as superscripts in both; one shared font size; text left-aligned (as scripture is today). The reference line at the bottom right reads `John 3:16–17 (KJV · NIV)`; under it, a small attribution line carries both translations' `bible_sources[].copyright` notices joined with " · " (missing or empty notices, such as KJV's, are left out; no line when both are empty). One above the other: main on top, a thin divider, the second below, each with its code at the left, text left-aligned.

**Lower-third and screen views.** A lower-third theme stacks both translations on every output. The one exception is FEAT-7's built-in **Lower third** stream screen view, which draws the primary translation only (FEAT-7 owns that per-output rule).

**Stage (foldback).** The stage shows the main translation only (FEAT-3 receives `SlideLayer::Text.text`, which is the main translation). `SlideLayer::Text.second` is available to a future stage cell. FEAT-3's stage notes key scripture slides by `NoteKey::Verse(VerseId)`, taken from the slide's `SlideSource.first` (§4).

**Phone remote.** Current/next cue text shows the main translation only. FEAT-5 adds nothing to FEAT-2's allowlist table.

### 3.3 Choose where the slides break

1. The volunteer selects a scripture item. The slides grid header (Plan 3 Task 7) shows a new button **Slide breaks** for scripture items (below 1500 px wide it sits in the header's single ⋯ menu with Choose/Print and Custom order, FEAT-1 §7.2). Item edit's Bible section also has a button **Choose slide breaks…** (it saves the item edits first, then opens the breaks editor).
2. The **Slide breaks** dialog (wide) shows on the left the verse list: every verse of either translation (the union, §5.4), each row with its verse number and the start of its text (main translation, or the second in italics when only the second has it). Between every pair of verses is a **gap line** drawn from `gap_state` (§5.4):
   - **Auto**: dashed accent line, "New slide (automatic)".
   - **Added**: solid accent line, "New slide (you added)".
   - **Kept**: faint line with a link icon, "Kept together".
   - **None**: faint line that shows "+ New slide here" on hover/focus.
3. Clicking a gap (or `Space` when it has focus; `↑`/`↓` move between gaps): None → Added → None; Auto → Kept → Auto; an Added gap that the planner would also break on returns to Auto when clicked.
4. The right side shows the resulting slides as thumbnails, rendered with the item's resolved theme at the planning canvas (§5.4), updated after each click (planned on a worker; a spinner shows on the thumbnails meanwhile). A slide that will be split inside a verse has a warning chip "Too long — will split inside a verse", and when the click just made such a slide, an inline note appears under the gap list (no confirm box): "Keeping these together makes a slide too long, so it will split inside a verse."
5. **Verses per slide** (Automatic / 1–4) is here as well (it stays in Item edit too). Added breaks and kept-together marks work with any setting.
6. Footer: **Reset to automatic** (removes all marks, asks nothing — Cancel undoes it), **Cancel**, **Save**.
7. **Save** sends `service/update` with only the changed keys (`breaks`, `per_slide`; FEAT-1 §5.3 merge rule). Kept marks on gaps that are no longer automatic are dropped before sending. If the item is live, the room does not change; FEAT-4's reconcile shows its change banner with **Update screen** (Command::Refresh), which lands on the slide that contains the same verse as the one on screen (FEAT-4 calls `slides::index_for_source`, §5.6).
8. If the item was removed meanwhile, Save shows the core's toast "That item is gone." and the dialog stays open (Plan 3 Task 18 behaviour).

## 4. Data model

All new fields have serde defaults; existing `data.json` files load unchanged. FEAT-5 adds no variant to a persisted enum, so it does not bump the data format (`store.rs` `SUPPORTED_FORMAT` is FEAT-1's; FEAT-1 sets 2 and FEAT-6 sets 3). `VerseText` (in `src/model.rs`) is not changed: verse ids are derived, never stored.

```rust
// src/model.rs

/// Canonical verse identity in the English (KJV-style) versification.
/// `book` is the 0-based index into bible::BOOKS. Derived by
/// scripture::verse_id_of(&VerseText.reference); never filled by the Bible session.
/// Serialized as [book, chapter, verse] to keep data.json small.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerseId(pub u8, pub u16, pub u16);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParallelLayout { #[default] SideBySide, Stacked }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SecondText {
    pub translation: String,
    /// Stored only when the translation's status is `offline`; empty otherwise
    /// (text then comes from the scripture text cache, §5.1a).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
        /// Stored only when `translation` has status `offline` (as today for KJV).
        /// Empty on disk for online or keyed translations (§5.1a).
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

/// The one Slide listing (shared with FEAT-3/FEAT-4); built with Slide::new.
/// There is no `key` field: FEAT-3 derives notes::key_of(&Slide) -> Option<NoteKey>,
/// and for scripture uses NoteKey::Verse(source.first).
pub struct Slide {
    pub label: String,
    pub content: SlideContent,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub section: Option<SectionRef>, // FEAT-4
    #[serde(default, skip_serializing_if = "Option::is_none")] pub source: Option<SlideSource>, // FEAT-5
}

pub enum SlideContent {
    Text {
        text: String,
        /// FEAT-1 rich text. Always empty for scripture.
        #[serde(default, skip_serializing_if = "Vec::is_empty")] styles: Vec<StyleSpan>,
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
    pub text: String,          // second translation's text for this slide (plain; no styles)
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

For a parallel slide, `SlideContent::Text.footer` carries the joined copyright notices (§3.2); for a single-translation slide it carries that translation's `bible_sources[].copyright` (none for KJV, so `None`). `live::SlideLayer::Text` gains `second: Option<ParallelText>` (default `None`). `render::cache::RenderKey` stays a struct; it gains `styles` (FEAT-1) and `second` (FEAT-5), so a parallel slide caches separately under `CacheKey::Slide(RenderKey)`. Scripture and `ParallelText` always have empty styles.

**Load fix-ups.** FEAT-5 adds no load hook of its own. Its fix-ups run inside FEAT-1's single `Data::after_load(&mut self) -> bool` (true means save), after the migration chain in `store::load`:

- **Restricted text out of data.json.** For each scripture item (main and second) whose translation's status is not `offline`, any stored verse text is moved into the scripture text cache (§5.1a, within its cap) and cleared from the item. Returns true when anything moved. When `bible_sources` is not known yet at load, only KJV and bundled/imported translations count as `offline`.
- **Second equal to main** is removed (`second = None`).
- **Upgrade toast flag.** Existing scripture items are re-planned by measurement instead of the 330-character estimate, so a service planned before the upgrade may show more or fewer slides. The first time the app starts on the new version with scripture in the service, a one-time toast says "Bible slides are now fitted to the screen, so some passages may break in different places." (flag `Data.notes_seen: BTreeSet<String>`, key `"feat5-measured-breaks"`).

`ParallelLayout` serializes as `"side_by_side"` / `"stacked"`. Old data has no verse ids to fill: `verse_id_of` derives them whenever they are needed. Labels it cannot parse give `None` (§5.7).

**Rollback.** An older RustyPresenter at the same data format that saves `data.json` drops `second`, `layout`, `breaks`, `bible` and `room_size` (its serde ignores unknown fields on load and does not write them back). Rolling back loses second translations and manual breaks; the references stay, and offline verse text stays. Items in online translations have no stored text, so an older build without the scripture text cache shows them empty until they are looked up again. The release notes say so. Newer-format data opens read-only with FEAT-1's banner.

**Validation in the core** (`service/add`, `service/update`): `second.translation` must differ from `translation` ("Pick a different second translation."); `breaks` entries that are not verses of the item (union of both translations, by `verse_id_of`) are dropped; a verse in both `before` and `join` keeps only `before`; `before` on the first verse is dropped; verse text sent for a non-`offline` translation is moved into the text cache and cleared from the item (§5.1a). Both actions merge by key (FEAT-1 §5.3): a missing key keeps the stored value, an explicit null clears an optional field (`second: null` turns the second off). This covers the new fields `second`, `layout` and `breaks`.

## 5. Core behaviour

### 5.1 Building on the Bible session's interface

The Bible session (branch `bible-esv`) owns everything under `src/bible*`. FEAT-5 defines no provider trait and adds nothing there. It builds on:

- **Readers** (all return `Result<(String /*label*/, Vec<VerseText>), String>`):
  - `bible::kjv().lookup(input)` — bundled KJV.
  - `bible::import::ImportedBible::lookup` — bundled or imported texts (WEB, BSB planned).
  - `bible::sources::lookup(root, tr, input)` — `async`; copyrighted translations through per-church API keys.
- **Actions:** `bible/lookup {reference, translation} -> {label, verses:[{reference, text}], translation}`, unchanged; `bible/key {translation, key}` (sent from Settings → Bible).
- **State:** `state_json` gains `"bible_sources": [{translation, status: "offline"|"online"|"needs_key"|"unavailable", provider, signup_url?, copyright?}]`. `bible::TRANSLATIONS` becomes data-driven; FEAT-5 reads the translation list, statuses and copyright notices only from `bible_sources`.

**FEAT-5's adapter** (`src/scripture/fetch.rs`):

```rust
pub async fn fetch(root: &Path, translation: &str, input: &str)
    -> Result<(String, Vec<VerseText>), String>;
```

It dispatches on the translation's `bible_sources` status:

| Status | Fetch | Settings → Bible wording |
| --- | --- | --- |
| `offline` | `kjv().lookup` for KJV, otherwise `ImportedBible::lookup` (run in `spawn_blocking`) | "Ready on this computer" |
| `online` | text cache hit → cached verses; otherwise `bible::sources::lookup(root, tr, input).await`, then the result is written to the text cache (§5.1a) | "Ready (needs internet)" |
| `needs_key` | `Err("NIV needs a key. Add one in Settings → Bible.")` | "Needs a key — Get one" (opens `signup_url`) plus a key field that sends `bible/key` |
| `unavailable` | `Err("NIV isn't available.")` | "Not available" |

Verse ids come from `scripture::verse_id_of(&VerseText.reference)` (`src/scripture/input.rs`), which parses labels like `"John 3:16"` and `"Psalm 23:1"` with the existing public `bible::find_book` and `bible::BOOKS` (read-only use). FEAT-6 (print) and FEAT-7 (Planning Center import) call the same `scripture::fetch`. FEAT-7's import preview stays pure: it checks reference syntax only (`bible::parse_reference`) and checks that the passage exists with `scripture::fetch` at import.

- Verses are keyed by the shared English versification. A source whose edition differs maps to it inside the Bible session.
- The per-item verse limit is `min(400, cache_cap(translation))` (§5.1a), so a single item never needs more text than the provider allows to be kept.

### 5.1a Storing verse text (copyright rule)

- Verse text for `offline` translations may be stored on the item, as today.
- Text for `online` or keyed translations is **never** written to `data.json`, backups, copies, FEAT-6 templates or "copy last week". Items keep `reference` and `translation`; the core clears `verses` for those translations on `service/add`, `service/update` and in `after_load`.
- The text lives in `scripture::TextCache`: in memory, plus one bounded file per translation, `<data root>/scripture-cache/<TR>.json` (not included in backups). The file is capped at the provider limit, `cache_cap(provider)`: ESV 500 verses; any other online provider uses the same conservative 500 until the Bible session reports a cap. Eviction is least-recently-used, with verses of the open service evicted last. When the open service needs more than the cap, the extra verses are held in memory only for the session.
- **On service open**, the core fetches (async, through `scripture::fetch`) every scripture item whose text is not cached. Chapter text shown in the search dialog's context view for an online translation (ESV search only) is held in memory and never written to the cache file.
- An item whose text cannot be fetched builds no slides; its slides grid shows "Couldn't get NIV text — check the internet." with a **Try again** button, and the core refuses to take it live (same message). FEAT-4's reconcile skips it.
- **Print** (FEAT-6) uses the item's stored text or the cached text, and fetches only on a miss.

### 5.2 Keyword search (`src/scripture/search.rs`, new, core)

**Search access** per translation, from its `bible_sources` status:

```rust
pub enum SearchAccess {
    /// Status `offline` (KJV, bundled/imported): FEAT-5 builds its own index.
    Local,
    /// ESV's own /search endpoint, only if the Bible session exposes it. Optional extra.
    EsvOnline,
    /// Everything else. Online translations are never indexed, pre-warmed or bulk-fetched.
    None,
}
```

Until the Bible session exposes an ESV search call, ESV reports `None`. The Settings chip reads "Search: Ready" (Local), "Search: online (ESV)" (EsvOnline) or "Search isn't available for NIV" (None).

**Query parsing** (`SearchQuery::parse(&str)`): split into terms and quoted phrases; normalise each word (Unicode lowercase; fold `’`/`'` and remove them; drop every character that is not a letter or digit). Empty after normalising → error "Type at least one word to search for.". More than 12 terms → only the first 12 count.

**Index** (`SearchIndex`, one per **local** translation):

- Built by reading every chapter in order through the local reader (`kjv().lookup` / `ImportedBible::lookup` with `"<Book> <n>"` for n = 1, 2, … until the lookup fails), ids from `verse_id_of`; the cancel flag is checked between chapters. If the Bible session later offers a verse iterator for local texts, the index uses it instead.
- `verses: Vec<VerseId>` (ordinal = Bible order), `texts: Vec<Box<str>>` (display text), `norm: Vec<Box<[u32]>>` (each verse as word ids, for phrase and proximity checks).
- `words: Vec<(Box<str>, Vec<u32>)>` sorted by word; each posting list is sorted verse ordinals.
- Lookup: a term of 3+ letters takes the union of postings for every word that starts with it (binary search for the prefix range); a shorter term matches its word exactly. All terms are intersected (AND).
- Quoted phrases: their words are intersected like terms; candidates are confirmed by scanning `norm` for the consecutive words. Phrases are filters.
- Scope filter: Old Testament = books 0–38, New = 39–65, One book = that book; applied as an ordinal range before intersection.
- **Ranking** (`rank(hits, query) -> Vec<u32>`, pure): each hit gets a tier: **1** the whole query's words appear consecutively in order (prefix rule per word, punctuation ignored), **2** all words fall inside a window of `2 × terms + 2` words, **3** the rest. Sort by (tier, ordinal). "Bible order" sorts by ordinal only. The first 200 are returned with the total.
- Example (bundled KJV, `be still and know`): three hits — Genesis 41:21 (tier 3), 1 Kings 22:3 (tier 3), Psalm 46:10 (tier 1). Best match lists Psalm 46:10 first.
- **Building.** Lazily, the first time a local translation is searched, in one `tokio::task::spawn_blocking` task holding a cancel flag. Pre-warm (local translations only): once the room output has committed its first frame (or 10 s after start-up when no room window is open), one task builds the index for the Bible box translation if it is local, otherwise for nothing; it is cancelled if a search starts for another translation, and restarted lazily later. At most one build runs at a time. Kept in memory for the app's lifetime, never written to disk. Target on a 2015 dual-core laptop: build ≤ 2 s, ≤ 15 MB per translation, query + rank ≤ 50 ms.
- While building, a search returns `Pending`; the dialog shows "Getting KJV ready to search…" and repeats the query when the build finishes (`Update::SearchReady(code)`).

For `EsvOnline`, the query is forwarded to ESV's search with the same limit and ESV's order is kept; hit text is shown but not written to the text cache. A network failure shows "Searching ESV needs the internet. Check the connection, or search in KJV." For `None`: "Search isn't available for NIV. Try a translation stored on this computer, like KJV."

**Snippet**: `snippet(text, query) -> String` returns up to 160 characters, starting at a word boundary at most 40 characters before the first match, with "…" where cut.

### 5.3 Actions (through `App::do_action`, as Plan 3 requires)

`bible/lookup` and `bible/key` are the Bible session's and are used unchanged. FEAT-5's actions use the `scripture/` prefix.

| Action | Body | Result |
| --- | --- | --- |
| `scripture/search` | `{query, translation, scope: "all"\|"ot"\|"nt"\|{"book":u8}, order: "best"\|"bible"}` | `{state:"ready", total, hits:[{id, reference, snippet}]}` or `{state:"pending"}`; error for a `None` translation |
| `scripture/chapter` | `{translation, book, chapter}` | `{verses:[{reference, text}]}` (via `scripture::fetch`) |
| `scripture/parallel` | `{reference, translation, second, second_required: bool}` | `{primary: <bible/lookup result>, second: <bible/lookup result> \| null, second_error?: String}`; a primary failure fails the whole action; with `second_required` a second failure fails it too ("Couldn't get NIV: …") |
| `settings/bible` | `{second_translation: Option<String>, layout}` | `{}`; rejects unknown names ("Unknown translation."), a translation that is not usable (its status wording), and second == current main ("Pick a different second translation.") |
| `settings/translation` (extended) | unchanged body | when the new main equals `bible.second_translation`, also sets it to `None`; result gains `second_turned_off: bool` |
| `displays/room_size` | `{w, h}` | `{}`; sent by the UI when the room window opens; ignored when unchanged |

None of these touch `Live`. FEAT-5 adds nothing to FEAT-2's phone allowlist table (`server.rs` keeps rejecting these). Without a second translation the Bible box sends `bible/lookup` as today. With one, it sends `scripture/parallel` with `second_required: false` (the church default may lapse); Item edit sends `second_required: true` (explicit choice). Both then use `service/add` / `service/update`.

**Settings save.** `settings/save` merges by key (FEAT-1 §5.3): the Settings dialog sends only the fields its tabs edit. The Bible tab edits `bible` only through `settings/bible` (applied at once) and keys only through `bible/key`, so Save never sends `bible` and never undoes a second-translation change made from the top bar or by another operator.

### 5.4 Grouping verses into slides (`src/slides.rs`, pure)

```rust
pub fn plan_scripture(input: &ScriptureInput, m: &mut dyn Measure) -> PlannedScripture;
pub struct PlannedScripture { pub slides: Vec<Slide>, pub notes: Vec<String>, pub auto_gaps: BTreeSet<VerseId> }
pub trait Measure { fn fits(&mut self, main: &str, second: Option<&ParallelText>) -> Fit; }
pub enum Fit { Comfortable, Shrunk, TooLong }
```

`ScriptureInput` holds the resolved verses of both translations (stored text or text cache, §5.1a) with ids from `verse_id_of`, `per_slide`, `layout`, `breaks`, translation codes, copyright notices, the resolved theme and the canvas. Slides are built with `Slide::new` and have `section: None`, empty `styles`.

- `Comfortable`: fits at a font size ≥ `READABLE = max(0.75 × or_default(theme.scripture_size, DEFAULT_SCRIPTURE_SIZE), 40)` px at 1080p scale (the resolved size, so `scripture_size = 0` gives 48).
- `Shrunk`: fits only between `MIN_PX` (26) and `READABLE`.
- `TooLong`: does not fit even at `MIN_PX`.

**Verse sequence.** Iterate the **union** of `VerseId`s from both translations in order. Each entry has main text, second text, or both. Verses without an id (labels `verse_id_of` cannot parse) keep their stored order after the previous id'd verse.

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

### 5.5 Planning off the command queue (`src/scripture/plan.rs`, new, core)

`build_item` runs synchronously in the command path (`DataPlan::build` in `App::command`, state building, and the phone snapshot in `server.rs`). It must never shape text or fetch.

```rust
pub struct PlanKey(u64);   // hash of resolved verses (both), breaks, per_slide, layout, codes, notices,
                           // resolved theme (all fields), canvas
pub struct PlanCache { map: RwLock<HashMap<Uuid, (PlanKey, Arc<PlannedScripture>)>> }
pub struct Planner { tx: SyncSender<PlanJob>, … }   // one thread "scripture-planner"
```

- **Planner thread.** One dedicated thread with its **own** `Fonts` (not shared with the render worker, so there is no lock contention with room rendering) and a `LayoutMeasure` that runs the real `layout()`; 4,096-entry LRU keyed by `(text hash, second, theme, canvas)`. Jobs are coalesced per item (latest wins). After every Data update and every text-cache fill, the app submits jobs for scripture items whose `PlanKey` changed; it also submits all scripture items when the planning canvas or the resolved theme changes. Completion writes `PlanCache` and emits `Update::PlansReady(Vec<Uuid>)`, on which FEAT-4's reconcile runs.
- **`build_item` only reads.** It reads text from the item or `TextCache` (memory only; a miss means "text missing", §5.1a), computes the `PlanKey` (hashing ~100 KB for a 400-verse parallel item: well under 1 ms) and uses the cached plan when the key matches. Otherwise it plans with `CharMeasure` (the deterministic character-budget estimate, microseconds) and marks the item `fitting: true` in state; the slides grid header shows a small chip "Fitting slides…". The command path never calls `LayoutMeasure`.
- **Canvas.** Plans use `displays.room_size` (last size the room window opened at), or 1920×1080 when never opened. Unplugging the projector or closing the room window changes nothing. When the room window opens at a different size, the UI sends `displays/room_size`, which re-plans all scripture items. A 4:3 projector therefore plans at 1024×768 once it has been opened once.
- **Before FEAT-1** the resolved theme is `settings.theme`. After FEAT-1 it is FEAT-1's resolver (item > service > church default), which falls back to the church default for a deleted theme id; the `PlanKey` includes the resolved theme, so the breaks re-plan automatically.

### 5.6 Live safety, navigation and the Bible box

- Search, chapter view, the breaks editor and Item edit never change the room. Only Take Live / thumbnail clicks / Next / Prev / FEAT-4's Update screen do, through existing commands with ids and base versions (v1 §5.4). `Command::Show { item_id, slide }` keeps its literal meaning: show that index.
- **Edited live item.** FEAT-5 adds no notice and no UI re-take mapping. FEAT-4 alone reconciles the live item (after `App::edit` and on `Update::PlansReady`, skipping scripture items whose plan is still pending) and shows its change banner with **Update screen** (Command::Refresh). For scripture, Refresh maps the position in the core by calling FEAT-5's mapper:
  `slides::index_for_source(old: Option<&SlideSource>, new: &[Slide]) -> usize` (pure, core): the new slide whose source range contains `old.first` with the same `part` if present, else part 0 of it; with no source, the same index clamped; fallback 0. `live.notice` holds only short-lived notices owned by other specs; FEAT-5 writes none.
- **Selection and arm across re-plans.** When an item's slides change (edit, plan finished, canvas change), the UI maps the selected thumbnail and the armed slide with the same `index_for_source`. Thumbnail clicks send the clicked index unchanged. Plan 3 Task 11's "Enter on an armed item sends slide 0" still holds for items that are not live.
- An item whose text is missing (§5.1a) cannot be taken live: the core rejects Show with "Couldn't get NIV text — check the internet."
- Clear matrix: unchanged. A parallel slide is an ordinary slide-layer frame; Clear Slide removes both columns. FEAT-1 overlays sit above it as on any slide.
- A failed main lookup leaves the service unchanged; the Bible box keeps its text and shows the core error.

**Bible box input** (`pub fn classify_bible_input(&str) -> BibleInput`, pure, in `src/scripture/input.rs`; uses the existing `bible::parse_reference` and `bible::find_book` read-only):

```rust
pub enum BibleInput { Reference(Reference), BookOnly { book: u8 }, UnknownWithDigits, Words }
```

1. `parse_reference` succeeds → `Reference` → look it up (unchanged).
2. Else the trimmed text is a book name or unique prefix (`find_book` on the whole text: "Job", "Mark", "judge", "numbers") → `BookOnly` → the existing error "Add a chapter, like Job 1 or Job 1:1." in the Bible box hint row, with a button **Search for "job" instead**.
3. Else the text contains any digit ("Hezekiah 3:1", "40 days", "seventy times 7") → `UnknownWithDigits` → the existing error "I don't recognize the book…" with a button **Search for these words**.
4. Else → `Words` → open Find a verse with the text. If the search finds nothing and the text is one word, the empty list says "No verses in KJV have that word. If it's a book name, check the spelling and add a chapter, like John 3." ("Phillipians" lands here).

`Ctrl+Enter` skips classification and always searches. The hint row sits under the Bible box (one line: message + one button, focusable with `Tab`), and disappears when the text changes. Keys follow FEAT-1 §7's key table; FEAT-5 adds only `Ctrl+Enter` inside the Bible box and keeps the existing `Alt+digit` chips.

### 5.7 Edge cases

- Search query that is only short words (`a of`): allowed, exact matches, capped at 200 shown.
- Typing while the index builds: "pending"; typing more replaces the waiting query.
- The per-item verse limit (`min(400, cache_cap)`) applies to Add from the context view as well.
- `per_slide = 1` with `join` marks: joins still merge verses.
- Second translation not usable or its lookup fails: Bible box adds the main only with the warning toast (§3.2); Item edit reverts the drop-down with the error.
- Main set equal to second: second turned off (§3.2).
- Online item offline at service open: "Couldn't get NIV text — check the internet." on the item, **Try again**; not taken live; other items unaffected.
- Text cache full: least-recently-used verses outside the open service are evicted first; extra verses for the open service are kept in memory only.
- A translation's key is removed (status becomes `needs_key`): cached text still shows existing items; new lookups fail with the key message.
- Room window opens at a new size while an item is live: live output unchanged; FEAT-4's banner appears if the live item's plan changed.
- Item references a deleted theme (FEAT-1): church default, re-planned.
- Verses whose label `verse_id_of` cannot parse: they plan by order, breaks can't be added on that verse (gap disabled, tooltip "This verse came from an older version; look the passage up again to choose breaks here.").

## 6. Rendering (`src/render`)

- `layout.rs`: `layout()` checks `slide.second`. When `Some`, it calls new `layout_parallel()`:
  - **Side by side:** body box split into two columns with a 4 % canvas-width gutter and a 2 px divider at 25 % opacity of the theme text colour. Each column starts with a heading (translation code, `REFERENCE_PX × 0.8`, weight 600, theme colour at 70 % alpha).
  - **Stacked:** main block on top, divider, second block below; headings at the left.
  - Text is **left-aligned** in both layouts, as single-translation scripture is today (`layout.rs` scripture branch).
  - One shared font size: `fit_steps` shrinks until **both** blocks fit; the larger block decides. Lower-third theme position (`position = "lower"`): stacked is used whatever the item says (the UI disables Side by side there, §3.2). FEAT-7's Lower third stream screen view drops `second` before layout and draws the primary only.
  - Reference line unchanged (bottom right). The attribution footer sits under it at `REFERENCE_PX × 0.7`; space for both is reserved first (v1: text may never overlap the reference/attribution area). When the footer is too wide it shrinks to `MIN_PX × 0.6` then wraps to two lines before the body loses any size.
- `TextLayout` gains `second_body: Option<Buffer>`, `second_box: Option<Rect>`, `headings: Option<(Buffer, Buffer)>`, `footer_box: Option<Rect>`; `raster.rs` draws them with the body's shadow rules.
- `RenderKey` gains `second`; the renderer cache key is `CacheKey::Slide(RenderKey)` (FEAT-1).
- `measure.rs` (new): `LayoutMeasure` (§5.5) and `pub fn fit_px(fonts, slide, theme, canvas) -> f32`. Used only by the planner thread and the breaks dialog worker.
- Thumbnails for the breaks editor and the Settings preview use the existing `Renderer` (cache + worker) at 384 px wide and the planning canvas aspect.

## 7. UI (extends Plan 3)

**Strings.** All user-facing text lives in `src/strings/scripture.rs` (`pub const`s and small `fn`s for strings with values), in the shared `src/strings/` tree (FEAT-4 §7 convention). Core errors and UI messages both come from there. Fixed labels in new `.slint` files use `@tr("…")`; text with values is passed in as properties from `strings::scripture`. Wording follows `web/operator.js`: short, plain, second person. The UI word for slide styles is "theme"; FEAT-5 uses no "Look" wording.

**Layout budget** (FEAT-1 §7.2, 1366×768): the second-translation chip folds into the translation chip menu below 1500 px wide; **Slide breaks** joins the slides header's ⋯ menu below 1500 px; the "Fitting slides…" chip sits in the slides header; Item edit's Bible section fits the scrollable Item edit. FEAT-5 adds nothing to the Live column.

| Plan-3 file | Change |
| --- | --- |
| `ui/operator/topbar.slint`, `src/ui/operator/bible.rs` | `+ NIV` second chip with X, `+` menu "Also show…" (usable translations only; folds into the chip menu below 1500 px), hint row with one action button, `Ctrl+Enter` = search; submit uses `classify_bible_input`; `scripture/parallel` when a second is set; main-only fallback toast; "Second translation turned off" toast; chips greyed with status wording from `bible_sources` |
| `src/ui/operator/` slides grid header (Task 7) | **Slide breaks** button for scripture items (⋯ menu when narrow); "Fitting slides…" chip; "Couldn't get NIV text — check the internet." with **Try again** for items whose text is missing |
| `src/ui/operator/` selection / keys (Tasks 8, 11) | selection/arm mapped across re-plans with `index_for_source`; sends `displays/room_size` when the room window opens |
| `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs` | **Bible** section: Translation, Second translation, Layout controls (usable only; lower-third note); **Choose slide breaks…**; Save sends only changed keys |
| `ui/dialogs/add.slint`, `src/ui/dialogs/add.rs` | Bible tab: **Search by words…** link; Enter classifies like the Bible box |
| Settings → Bible tab (Task 20; FEAT-1 §7 tab table) | Per-translation rows from `bible_sources`: name, status wording (§5.1), "Get one" link to `signup_url` and a key field sending `bible/key` for `needs_key` (also shown to replace a key), search chip ("Search: Ready" / "Search: online (ESV)" / "Search isn't available for NIV"), copyright notice. Below: Second translation drop-down, Layout segmented (lower-third note), preview thumbnail; applied through `settings/bible` |
| Settings Help tab | The search rules from §3.1 and the breaks editor keys |
| `src/ui/dialogs/mod.rs` | `DialogKind::BibleSearch { query: String }`, `DialogKind::ScriptureBreaks(Uuid)`; both are **not** refreshed by `dialog_refresh` (they hold user input) |

**New files:**

- `ui/dialogs/bible-search.slint` — `BibleSearchDialog`: `in-out property <string> query`, `in property <[SearchHitRow]> hits` (`{reference, snippet}`), `in property <string> status`, `in property <[ContextVerse]> context` (`{number, text, hit: bool, selected: bool}`), `in property <string> add-label`, `in property <[string]> translations`, `in property <[bool]> translation-searchable`, `in-out property <int> translation-index`, `in-out property <int> scope-index`, `in-out property <bool> bible-order`, `in property <[string]> books`, `in-out property <int> book-index`; callbacks `query-edited()`, `query-submit()`, `pick-hit(int)`, `pick-verse(int, bool /*extend*/)`, `extend(int /*-1|+1*/)`, `add()`.
- `src/ui/dialogs/bible_search.rs` — debounce (250 ms `slint::Timer`; `query-submit` fires it at once), stale-result guard (each request carries a sequence number; older replies are dropped), range selection model (`Selection { anchor, head }`, pure, clamped to the chapter), add flow reusing the Bible box insert/arm/toast code (factored into `add_scripture_item(translation, second: Option<String>, …)`).
- `ui/dialogs/scripture-breaks.slint` — `ScriptureBreaksDialog`: `in property <[GapRow]> rows` (`{verse-number, preview, second-only: bool, gap: string /*"none"|"auto"|"added"|"kept"*/, enabled: bool}`), `in property <[Thumb]> slides` (`{image, label, warn}`), `in property <string> note`, `in property <bool> busy`, `in-out property <int> per-slide`; callbacks `toggle-gap(int)`, `reset()`, `save()`.
- `src/ui/dialogs/scripture_breaks.rs` — works on a clone of the item; `fn click(state: GapState) -> Mark` (pure); re-plans with `plan_scripture` + its own `LayoutMeasure` in `spawn_blocking` after each click (stale results dropped by sequence number) and renders thumbnails through `Renderer`; Save drops stale `join` marks then sends `service/update` with the changed keys.

**New core files** (none under `src/bible*`): `src/scripture/mod.rs`, `src/scripture/fetch.rs` (`fetch`, `TextCache`, `cache_cap`), `src/scripture/search.rs` (`SearchAccess`, `SearchQuery`, `SearchIndex`, `rank`, `snippet`), `src/scripture/input.rs` (`classify_bible_input`, `parse_verse_label`, `verse_id_of`), `src/scripture/parallel.rs` (`scripture/parallel` handling, second-translation rules), `src/scripture/plan.rs` (planner, `PlanCache`), `src/strings/scripture.rs`. `plan_scripture`, `gap_state` and `index_for_source` live in `src/slides.rs`.

All colours and fonts come from the `Theme` global. Every control has an accessible name so the testing backend can find it.

## 8. Errors

All messages live in `src/strings/scripture.rs`.

| Situation | Message | Effect |
| --- | --- | --- |
| Empty search | "Type at least one word to search for." | Nothing searched |
| No hits | "No verses in KJV have all of those words." | Empty list, context cleared |
| No hits, one word | "No verses in KJV have that word. If it's a book name, check the spelling and add a chapter, like John 3." | Empty list |
| Too many hits | "Showing the first 200 of 1,240 verses — add more words to narrow it down." | First 200 shown |
| Index building | "Getting KJV ready to search…" | Retries automatically |
| Translation without search | "Search isn't available for NIV. Try a translation stored on this computer, like KJV." | Nothing searched |
| Bible box translation not searchable | "Search isn't available for NIV, so this searches KJV." | Dialog opens on KJV |
| ESV online search offline (only if offered) | "Searching ESV needs the internet. Check the connection, or search in KJV." | Nothing searched |
| Book name alone | "Add a chapter, like Job 1 or Job 1:1." + **Search for "job" instead** | Hint row |
| Digits, unknown book | existing "I don't recognize the book…" + **Search for these words** | Hint row |
| Translation needs a key | "NIV needs a key. Add one in Settings → Bible." | Lookup fails |
| Translation not available | "NIV isn't available." | Lookup fails |
| Online text not reachable | "Couldn't get NIV text — check the internet." | Item shows Try again; not taken live |
| Second = main (item) | "Pick a different second translation." | Item unchanged |
| Main changed to second | "Second translation turned off (same as main)." | Toast; second off |
| Church second not usable / failed | "NIV isn't available right now, so only KJV was added. Turn off the second translation with the X." | Main-only item added, warning toast |
| Item-edit second lookup fails | "Couldn't get NIV: `<core message>`" | Drop-down reverts |
| Missing verse in one translation | "NIV doesn't include Acts 8:37." | Operator note on the item |
| Verse too long | "Too long — will split inside a verse" | Warning chip on the thumbnail |
| Join made a slide too long | "Keeping these together makes a slide too long, so it will split inside a verse." | Inline note in the breaks dialog |
| Item removed while editing breaks | "That item is gone." (core, Plan 3) | Toast; dialog stays open, Save still active |
| First start after upgrade | "Bible slides are now fitted to the screen, so some passages may break in different places." | One-time toast |

Settings → Bible status wording ("Ready on this computer", "Ready (needs internet)", "Needs a key — Get one", "Not available") and the search chips also live in `strings::scripture`. The edited-live-item banner wording is FEAT-4's.

### 8.1 Note on highlighting

Slint 1.18 `Text` cannot style part of a string. Results show the trimmed snippet and the context view highlights the whole hit verse. Word highlighting can come later without changing the core API (`hits` can gain match ranges).

## 9. Testing

**Unit tests (core):**

- `scripture/search.rs`: `query_parses_terms_and_quoted_phrases`, `query_normalises_case_and_apostrophes`, `empty_query_is_an_error`, `prefix_matches_loveth_and_loved`, `short_terms_match_exactly`, `all_terms_must_match`, `phrase_requires_consecutive_words`, `scope_old_testament_excludes_john`, `scope_one_book`, `bible_order_option_sorts_by_ordinal`, `rank_tiers_phrase_then_proximity_then_rest`, `be_still_and_know_ranks_psalm_46_10_first` (full bundled KJV; also asserts exactly 3 hits), `caps_at_200_and_reports_total`, `snippet_trims_around_first_match`, `search_access_local_only_for_offline_status`, `online_translation_never_indexed_or_prewarmed` (a fake `sources` reader that panics on any call during build/pre-warm), `prewarm_skips_online_bible_box_translation`, `prewarm_cancels_when_other_translation_searched`, `index_build_under_two_seconds_release` (`#[ignore]`, run manually on the old laptop).
- `scripture/input.rs`: `verse_id_of_round_trips_display_names` (all 66 books, "Psalm" singular), `verse_id_of_parses_labels_from_kjv_and_imported_readers`, `unparseable_label_gives_none`, `classify_reference`, `classify_book_only_job_mark_judge_numbers`, `classify_digits_unknown_book`, `classify_words`.
- `scripture/fetch.rs`: `fetch_dispatches_by_status` (offline → local reader, online → sources, needs_key / unavailable → their messages), `online_fetch_writes_cache_and_hits_it_next_time`, `cache_file_capped_at_500_verses_for_esv`, `cache_evicts_outside_open_service_first`, `per_item_limit_is_min_of_400_and_cap`, `copyright_comes_from_bible_sources`.
- `model.rs` / `store.rs`: `old_scripture_item_loads_with_defaults`, `settings_without_bible_section_loads`, `parallel_layout_serializes_snake_case`, `verse_breaks_is_empty_skips_serializing`, `online_verse_text_never_in_data_json` (save, backup, copy and template paths), `service_add_moves_online_text_to_cache`, `after_load_moves_stored_online_text_to_cache_and_returns_true`, `after_load_removes_second_equal_to_main`, `upgrade_toast_shown_once`, `no_format_bump_for_feat5_fields`.
- `slides.rs`: `auto_groups_by_measure_not_chars`, `manual_break_before_starts_new_slide`, `join_overrides_auto_break`, `join_too_long_splits_inside_verse_with_cont_label`, `per_slide_n_with_manual_break`, `before_on_first_verse_is_ignored`, `parallel_pairs_verses_by_id`, `parallel_missing_verse_leaves_column_empty_and_notes_it`, `label_includes_verse_only_in_second_translation`, `parallel_long_verse_splits_both_into_same_part_count`, `reference_line_names_both_translations`, `discontiguous_labels_stay_exact`, `slide_source_set_for_every_slide`, `scripture_slides_have_empty_styles_and_no_section`, `breaks_move_to_nearest_existing_boundary_on_retranslate`, `gap_state_table` (all 8 rows), `auto_gaps_ignore_join_marks`, `readable_uses_resolved_scripture_size`, `index_for_source_same_first_verse`, `index_for_source_falls_back_to_zero`.
- `scripture/plan.rs`: `build_item_uses_cached_plan_when_key_matches`, `build_item_falls_back_to_char_measure_and_marks_fitting`, `build_item_with_missing_online_text_has_no_slides`, `plan_key_changes_with_theme_breaks_canvas_second`, `deleted_item_theme_replans_with_church_default` (after FEAT-1; before it, `theme_change_replans`), `unplugged_room_keeps_last_known_canvas`, `room_size_change_replans_all_scripture`, `planner_coalesces_jobs_per_item`.
- `app.rs`: `take_live_never_measures` (a `Measure` that panics when a thread-local "in command" flag is set by `App::command`; Show, Next, state building and the phone snapshot run on a 400-verse parallel item), `next_on_400_verse_parallel_item_is_fast` (command handling under 20 ms in release, `#[ignore]` in debug), `scripture_search_action_never_touches_live`, `scripture_parallel_optional_second_returns_second_error`, `scripture_parallel_required_second_fails_whole`, `bible_lookup_unchanged_shape`, `service_update_rejects_same_second_translation`, `service_update_drops_unknown_break_verses`, `service_update_merges_by_key_and_null_clears_second`, `settings_bible_rejects_unknown_translation`, `settings_bible_rejects_needs_key_translation`, `settings_bible_rejects_same_as_main`, `picking_main_equal_to_second_turns_second_off`, `show_refused_for_item_with_missing_text`, `refresh_maps_scripture_by_index_for_source` (with FEAT-4), `remote_cannot_call_scripture_search` (server allowlist).
- `live.rs`: `show_uses_the_given_index`, `editing_breaks_of_live_item_keeps_committed_frame`.
- `render`: `parallel_columns_share_one_font_size`, `stacked_forced_in_lower_third`, `lower_third_stream_view_draws_primary_only` (with FEAT-7), `parallel_text_left_aligned_in_both_layouts`, `parallel_slide_shows_both_attributions`, `attribution_shrinks_before_body`, `parallel_render_key_differs_from_single`, `layout_measure_caches_repeat_calls`.

**UI tests (Slint testing backend, `Ops::inline`, Plan 3 helpers):**

- Search: `bible_box_words_open_search_dialog_without_adding`, `bible_box_unknown_book_with_digits_keeps_error`, `single_word_book_name_offers_search`, `words_with_digits_offer_search`, `ctrl_enter_always_searches`, `online_bible_box_translation_searches_kjv_with_note`, `search_typing_debounces_and_shows_hits`, `enter_in_search_box_focuses_first_result`, `search_down_arrow_enters_results`, `shift_click_extends_range_and_add_label_updates`, `shift_down_stops_at_chapter_end`, `search_enter_adds_after_live_and_arms`, `search_add_uses_dialog_translation_and_church_second`, `second_enter_takes_search_result_live`, `f1_in_search_dialog_clears_and_dialog_stays`, `stale_search_reply_is_ignored`, `unsearchable_translation_greyed_in_search_dropdown`.
- Two translations: `second_chip_x_turns_second_off_and_saves`, `plus_menu_lists_only_usable_translations`, `second_chip_folds_into_chip_menu_below_1500px`, `bible_box_adds_parallel_item`, `unusable_second_adds_main_only_with_warning`, `alt_digit_equal_to_second_shows_turned_off_toast`, `save_settings_does_not_send_bible`, `side_by_side_disabled_for_lower_third_theme`.
- Settings → Bible: `status_rows_follow_bible_sources`, `needs_key_row_shows_get_one_and_key_field_sends_bible_key`.
- Item edit: `item_edit_bible_section_change_translation_relooks_up`, `item_edit_failed_lookup_reverts_dropdown`, `item_edit_layout_hidden_without_second`.
- Breaks: `breaks_dialog_click_gap_cycles_states`, `breaks_dialog_space_toggles_focused_gap`, `breaks_dialog_reset_then_cancel_keeps_original`, `breaks_dialog_join_too_long_shows_note`, `breaks_dialog_save_drops_stale_joins`, `breaks_dialog_save_on_live_item_shows_feat4_banner`, `breaks_dialog_save_on_removed_item_shows_item_gone`.
- Live: `update_screen_on_scripture_lands_on_same_first_verse` (FEAT-4 banner), `thumbnail_click_sends_literal_index`, `canvas_change_maps_selection_by_source`, `fitting_chip_shows_until_plan_ready`, `missing_text_item_shows_try_again`.

**Manual checks (church laptop and one old laptop, Windows; one macOS or Linux machine):**

1. Search "be still and know" → Psalm 46:10 first in Best match, third in Bible order; results under a second after the first search.
2. Two translations side by side on the projector from the back row: both columns readable for John 3:16–18 at the default theme. Repeat stacked and with a lower-third theme.
3. Romans 8:28–39 in parallel mode: no slide text below readable size without a warning; no overlap with the reference or attribution lines.
4. Add a manual break while the passage is live: room unchanged, FEAT-4's banner shows; Update screen lands on the same verse.
5. Old laptop: first search time; memory in Task Manager before/after indexing; press Next rapidly through a freshly added 150-verse parallel passage — no stutter, Clear (F1) immediate.
6. Unplug the projector mid-service and plug it back: slide counts unchanged.
7. With an ESV key: add an ESV passage, save, open `data.json` and a backup — no ESV verse text; restart offline — the passage still shows from the cache; clear the cache and restart offline — "Couldn't get ESV text — check the internet." and it cannot be taken live.

## 10. Dependencies on other FEAT specs

- **Plan 3 (required first):** Theme, Ops, dialogs, Live column, Item edit (Task 18), Settings (Task 20), keys (Task 11).
- **Bible session (`src/bible*`, branch `bible-esv`; required for anything beyond KJV):** provides `kjv().lookup`, `ImportedBible::lookup`, async `sources::lookup(root, tr, input)`, the data-driven translation list, `bible_sources` in state, `bible/lookup` and `bible/key`. FEAT-5 adapts to it through `scripture::fetch` and derives verse ids itself. Asks of that session: report a per-provider cache cap if it differs from 500; optionally expose ESV `/search` for `SearchAccess::EsvOnline`. Until it lands, only KJV is usable and other translations show their status wording.
- **FEAT-1 (optional):** before FEAT-1, measurement and rendering use `settings.theme`; after it, FEAT-1's resolver (item > service > church default, deleted id → church default). If FEAT-1 adds a minimum readable size to `Theme`, `READABLE` uses it. FEAT-1 owns `SUPPORTED_FORMAT`, the migration chain, `Data::after_load` (FEAT-5's fix-ups run inside it), the settings/service merge-by-key rule, the key table and Settings tab table (§7, Bible tab), the layout budget (§7.2), `StyleSpan`/`styles` (empty for scripture) and `CacheKey`. Overlays render above parallel slides with no special case.
- **FEAT-2:** owns the phone allowlist table (§5.11); FEAT-5 adds nothing to it.
- **FEAT-3 (Stage):** no stage work. The stage receives the main translation in `SlideLayer::Text.text`. FEAT-3's `NoteKey::Verse(VerseId)` uses FEAT-5's `VerseId` from `SlideSource.first`.
- **FEAT-4:** owns `Slide::new`, `SectionRef`, the live-item reconcile, the change banner and Command::Refresh. FEAT-5 provides `slides::index_for_source` as the core mapper Refresh calls for scripture, and emits `Update::PlansReady` (reconcile skips scripture items whose plan is pending). Owns the `src/strings/` convention.
- **FEAT-6 (Service planning):** templates and "copy last week" copy scripture items with `reference`, `translation`, `second`, `layout` and `breaks`; online text is never copied (§5.1a). Printed orders list `John 3:16–18 (KJV · NIV)` and both copyright notices from `bible_sources`, using stored or cached text and calling `scripture::fetch` only on a miss. Practice mode needs nothing from FEAT-5.
- **FEAT-7 (Livestream, CCLI, PCO):** screen views decide what each output shows; a lower-third theme stacks both translations everywhere except FEAT-7's built-in Lower third stream screen view, which draws the primary only. Scripture is not in the CCLI song usage report. PCO import checks reference syntax in its pure preview and calls `scripture::fetch` at import.

## 11. Decisions for Jake to review

1. **Words in the Bible box open search.** Default: typing words (no recognised reference, no digits, not a book name) and pressing Enter opens Find a verse with results; a book name alone or text with digits keeps its error but offers a "Search for…" button; `Ctrl+Enter` always searches. Alternative: keep the Bible box for references only and use a separate search button.
2. **Search matching.** Default: every word must appear; words match their beginnings (love → loved, loveth); quotes for exact phrases. Alternative: any word may match, ranked by how many match.
3. **Result order.** Default: best match first (exact words in order, then words close together, then the rest in Bible order), with a "Bible order" toggle. Alternative: Bible order only (Genesis to Revelation).
4. **Result limit.** Default: show the first 200 and say how many there are. Alternative: show all, however many.
5. **No search on the phone.** Default: the phone remote stays as it is. Alternative: add a read-only search to the phone page (that would add to FEAT-2's allowlist table).
6. **Search index kept in memory only.** Default: built when first needed (and quietly after the room is up, for your usual translation if it is stored on this computer), never saved to disk. Alternative: save the index to disk for an instant first search.
7. **Second translation off by default.** Default: one translation unless a church turns on a second in Settings, the top bar or an item. Alternative: ask during first-run setup.
8. **Top-bar X and + change the church default.** Default: turning the second translation on or off in the Bible box is saved for everyone until changed back (like the main translation chip). Alternative: they apply to the next passage only and reset afterwards.
9. **Second translation that isn't available.** Default: the Bible box adds the main translation alone and shows a warning toast. Alternative: refuse to add anything until the second is turned off.
10. **Main translation picked equal to the second.** Default: the second is turned off, with a toast. Alternative: swap them (the old main becomes the second).
11. **Side by side is the default layout.** Default: two columns, same text size, text left-aligned. Alternative: one above the other by default (better for very long passages and narrow screens).
12. **Lower-third themes always stack.** Default: with the lower-third text position, two translations show one above the other on every output and "Side by side" is greyed out; FEAT-7's Lower third stream screen view shows the main translation only. Alternative: allow side by side there too.
13. **Stage and phone show only the main translation.** Alternative: stage shows both (main, then second).
14. **Both copyright notices on every parallel slide.** Default: a small line under the reference with both notices (from each source's copyright text). Alternative: notices on the first slide of the passage only (only if the licences allow it).
15. **Missing verses.** Default: show the verse in the translation that has it, leave the other column empty for it, list it in the label, and tell the operator. Alternative: hide the verse from both sides.
16. **Breaks stay on verse boundaries.** Default: you can add a break between verses or keep two verses together; only the automatic splitter breaks inside a very long verse. Alternative: let volunteers break anywhere inside a verse.
17. **Break marks sit on top of automatic breaks.** Default: "New slide here" and "Kept together" adjust the automatic breaks; a "Kept together" mark is dropped when the automatic break under it goes away. Alternative: the first click turns automatic off for that item and every break becomes manual.
18. **"Readable size" for automatic breaks.** Default: a new slide starts before text would shrink below 75 % of the scripture text size (never below 40 px on a 1080p screen). Alternative: fill each slide as full as possible down to the smallest size (fewer slides, smaller text).
19. **No automatic break at chapter boundaries.** Default: John 1:51–2:2 can share a slide. Alternative: always start a new slide at a new chapter.
20. **Slides are fitted to the last-known room screen size.** Default: the size the room window last opened at (1920×1080 until then); unplugging the projector changes nothing. Alternative: fit to whatever screen is attached right now.
21. **Existing passages are re-fitted after the upgrade.** Default: old scripture items use the new measured breaks, with a one-time toast. Alternative: keep the old character estimate for items added before the upgrade.
22. **Edited live item uses FEAT-4's banner (changed).** Default: editing the live item (breaks, translation, songs too) leaves the room as it is and shows FEAT-4's change banner with "Update screen", which keeps your place by verse. FEAT-5 no longer has its own "Take Live to update" notice. Alternative: none proposed; the single mechanism is FEAT-4's.
23. **"Choose slide breaks…" in Item edit saves first.** Default: the button saves your Item edit changes, then opens the breaks editor. Alternative: open the breaks editor without saving and save both together (more complex).
24. **Reset to automatic asks no question.** Default: it resets in the dialog; Cancel brings everything back. Alternative: a confirm box.
25. **Search only for Bibles stored on this computer (new).** Default: keyword search works for KJV and bundled or imported Bibles; online translations (NIV, ESV and others) can't be searched because their owners forbid indexing; typing words while an online translation is chosen searches KJV with a note. ESV's own online search is an optional extra if the Bible session offers it. Alternative: none that the licences allow.
26. **Online translations' text is not saved with the service (new).** Default: services keep the reference and translation; the words are fetched when the service opens and kept in a small cache (ESV: at most 500 verses). Without internet and without a cached copy, the item says "Couldn't get NIV text — check the internet." and can't be shown. Alternative: none that the licences allow; the choice is only the cache size, which is capped by each provider.
27. **Passage length limit follows the provider (new).** Default: one item can hold up to 400 verses, or the provider's cache limit if that is lower. Alternative: always 400 (would break ESV's terms for very long passages).
28. **Keys entered in Settings → Bible (new).** Default: each translation shows its status ("Ready on this computer", "Ready (needs internet)", "Needs a key — Get one", "Not available"); "Get one" opens the provider's sign-up page and a key box saves the church's key. Alternative: a separate key setup wizard.
29. **Which translations a release needs (raised separately).** The earlier "all six translations" release gate is removed from this spec; the list is whatever the Bible session provides. Jake decides the release gate.

## Review notes

- FEAT-3 dependency: resolved by making FEAT-5 match FEAT-3 (stage shows main only) rather than editing FEAT-3, so the two specs agree without a cross-spec change.
- Strings convention: settled. All user-facing text lives in `src/strings/<area>.rs`; FEAT-5's module is `src/strings/scripture.rs`.
- Breaks model: kept two marks over automatic breaks (now fully defined by `gap_state`) instead of the fully manual model; the alternative is Decision 17.

## Cross-spec alignment (Nov 1)

Changes made to match the cross-spec rulings and the Bible session's interface:

- **Bible interface.** Dropped the `TranslationProvider` trait and `bible::provider`; FEAT-5 adds nothing under `src/bible*`. New adapter `scripture::fetch(root, translation, input)` dispatches by `bible_sources` status over `kjv().lookup`, `ImportedBible::lookup` and async `sources::lookup`. Translation list and copyright come from `bible_sources`; "all six translations" wording removed (release gate is with Jake; the app is free for every church).
- **Verse ids.** `VerseText` is unchanged (no stored `id`, no `fill_verse_ids`); ids come from `scripture::verse_id_of(&reference)`.
- **Files moved** out of `src/bible*`: `src/scripture/{fetch.rs, search.rs, input.rs, parallel.rs, plan.rs}`; strings in `src/strings/scripture.rs` (was `strings/bible.rs`).
- **Search is local only.** `SearchAccess { Local, EsvOnline (optional), None }`; online translations are never indexed, pre-warmed or bulk-fetched; chip wording "Search: Ready" / "Search: online (ESV)" / "Search isn't available for <tr>"; "Searching NIV needs the internet" removed.
- **Actions.** `bible/lookup` used unchanged; new `scripture/parallel` for two translations; `bible/search`/`bible/chapter` renamed `scripture/search`/`scripture/chapter`; `bible/key` used from Settings → Bible.
- **Storage rule.** Online/keyed verse text never in `data.json`, backups, copies or templates; bounded per-translation cache (ESV 500 verses); fetched on service open; "Couldn't get <tr> text — check the internet." and not taken live; per-item limit `min(400, cap)`; print uses stored/cached text.
- **Settings → Bible.** Status wording per translation, "Get one" link, key field sending `bible/key`.
- **Load fix-ups** run inside FEAT-1's `Data::after_load(&mut self) -> bool`; no FEAT-5 hook, no format bump.
- **Merge rule.** Replaced "Save copies `bible` from the latest snapshot" with FEAT-1's merge-by-key rule for `settings/save` and `service/update` (covers `second`, `layout`, `breaks`).
- **Edited live item.** Dropped FEAT-5's live notice and UI re-take; FEAT-4's reconcile/banner/Refresh handles it, calling `slides::index_for_source` as a core mapper. Decision 22 changed.
- **Slide / RenderKey.** Full `Slide` listing with FEAT-4 `section` and FEAT-5 `source`, no `key`; `SlideContent::Text` lists `styles` (empty for scripture); "runs replace text" dropped; `RenderKey` gains `second` under `CacheKey::Slide`. FEAT-3 uses `VerseId` via `SlideSource`.
- **Words, keys, layout.** "Looks" → themes/screen views; clear keys F1–F6 per FEAT-1 §7; phone: "adds nothing to FEAT-2's table"; second chip folds into the chip menu below 1500 px; Slide breaks in the ⋯ menu when narrow; Item edit Bible section.
- **Lower third.** Lower-third themes stack on every output except FEAT-7's Lower third stream screen view (primary only). Decision 12 updated.
- **New decisions** 25–29 (local-only search, online text not saved, provider-capped passage length, key entry in Settings → Bible, release gate with Jake).
