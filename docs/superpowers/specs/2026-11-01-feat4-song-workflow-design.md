# FEAT-4 — Song workflow: coloured sections, jump keys, live editing, arrangement drag-to-reorder

**Status:** Design (November 1, 2026), revised after review the same day. Written while Jake was asleep; every product choice made without him is listed in §11.
**Roadmap:** `PROJECT_PLAN.md` §10b, FEAT-4. Depends on Plan 3 (native operator window). Can be built before FEAT-1..3.
**Controls:** the v1 spec (`2026-10-29-rustypresenter-v1-design.md`) still controls live behaviour: layers and clear matrix (§6), command ids and base versions (§5.4), cues and navigation (§7), phone remote allowlist (§12.3). This spec adds to them and **amends exactly one rule**, the v1 §7.2 row "Live item edited in library" (§5.7). Nothing else in v1 changes.

---

## 1. Purpose and who it's for

Worship does not always follow the plan. The leader repeats the chorus, skips a verse, or adds a tag at the end. Today the operator has to hunt through thumbnails to find the right slide while the room waits. And when someone spots a typo in a lyric during the service, the only fix is to edit the song and then retake it, which jumps the screen back to slide 1.

This feature is for the volunteer at the laptop (often a teenager or a retiree, rarely the same person every week) and the worship leader who sometimes drives from a phone. It gives them:

1. **Coloured sections.** Every lyric slide shows which section it belongs to (Verse 1, Chorus, Bridge…) with a colour per kind, in the operator, on the phone and on the stage display.
2. **Jump keys.** One key jumps the live song to a section: `C` chorus, `B` bridge, `1`–`9` verse N, and so on. The phone gets the same as tappable chips.
3. **Safe live editing.** Fix words in a song that is on screen. Nothing on the room screen changes until the operator says so, and when it does, the screen stays on the same place in the song.
4. **Arrangement drag-to-reorder.** The song's order (V1 C V2 C B C) is a row of coloured chips you drag around, not a comma-separated text box. A service can use its own order for a song without changing the song's usual order.

## 2. Scope

**In:**

- Section kind detection from existing labels; a fixed colour per kind; section info on every song slide.
- Jump command in the core (`Command::Jump`), operator keys (with a Settings switch), section bar, phone chips, phone allowlist entry.
- Live-item reconciliation after any change to library or service data: quiet adoption of new slides when the screen is unaffected, a "changed" banner plus an explicit **Update screen** command when it is.
- "Edit words" from the live column, with the cursor at the live section; **Save and show now**.
- Arrangement strip widget (drag, keyboard move, add, remove, reset) in the song editor and in Item edit (per-service order override, with a visible "Custom order" badge).
- Persistence of the per-service order with serde defaults.

Build order inside FEAT-4 (each step ships on its own): (a) sections, colours, jump keys, phone chips; (b) reconcile, Refresh, Edit words; (c) drag-reorder of the song's usual order; (d) per-service override. Step (d) is behind decision D10: if Jake says no, it is dropped without touching (a)–(c).

**Out (not in this spec):**

- User-editable section colours or user-defined jump keys (§11 D2, D4).
- Editing slide text directly on the slide thumbnail (we always edit the song's lyrics; §11 D7).
- Song key/chord charts, transposition, musician view.
- Section colours on the room output (the congregation never sees them).
- Stage layout design (FEAT-3 places the section element; this spec provides the data).
- Scripture slide-break editing (FEAT-5) and rich text in lyrics (FEAT-1).
- Undo history for song edits (the editor's own Cancel is the undo; §11 D9).

## 3. User experience

### 3.1 Seeing sections

- **Slide grid (operator middle column).** Each song thumbnail has a 4 px colour band along its top and a small chip at the bottom-left of its caption: short name + label, e.g. `C  Chorus`, `V2  Verse 2`, `B  Bridge (2/2)`. The colour never stands alone: the short name is always printed.
- **Section bar.** Above the grid, when the selected item is a song, one chip per occurrence in played order: `V1  C  V2  C  B  C  E`. Each chip has its kind colour.
  - When the selected item **is the live item**, the chips come from the live snapshot (`LiveView.sections`, §4.2), the live occurrence is outlined in `Theme.live`, the tooltip is "Verse 2 — press 2" (only when Section keys are on), and a click sends `Jump { occurrence }` (it walks the same slides Next walks).
  - When the selected item is **not** live, the chips come from the library build, no key hints are shown, the tooltip is "Click to show this part", and a click takes the first slide of that occurrence live (`Show`, the same as clicking that thumbnail, Plan 3 Task 7).
- **Live column.** Under the live preview, the current section name and colour: "Chorus · slide 2 of 3". Up next shows the next cue's section chip too.
- **Service list.** A song item with a per-service order shows a small "Custom order" badge (`Chip`, no colour state) after its title; the slide header shows "Custom order for this service" in `Theme.muted`.
- **Phone.** Under the current-slide text, a scrollable row of chips for the live song's occurrences (from the snapshot), the live one highlighted.
- **Stage display.** The stage view model gets the current and next section (label + colour). The default stage layout shows the label above the current text in the section colour (FEAT-3 owns where; until FEAT-3 lands, `ui/stage.slint` adds one label line).

### 3.2 Jumping with keys (operator)

Keys work only when focus is not in a text field and no dialog or ask overlay is open (same as `→`/`←`, Plan 3 Task 10), and only when **Settings → Presenting → Section keys (C, B, 1–9…)** is on (default on). They always act on the **live** song, never on the selected (preview) item.

| Key | Jumps to |
| --- | --- |
| `1`–`9` | Verse 1–9 (see "unnumbered verses" below) |
| `V` | the next verse after the current position (any number) |
| `C` | Chorus (any numbered chorus: Chorus, Chorus 2…) |
| `P` | Pre-chorus |
| `B` | Bridge |
| `T` | Tag |
| `I` | Intro |
| `E` | Ending / Outro / End / Coda |
| `L` | Interlude / Instrumental / Turnaround / Vamp |

Upper- and lower-case are the same (Shift and Caps Lock do not matter). A key with Ctrl, Cmd or Alt held is not a jump key. `0` and the other letters do nothing (they stay reserved, §10).

**Every successful key jump shows a toast** naming the effect: "Jumped to Chorus (C)", "Jumped to Verse 2 (2)". This is the safety net for stray typing: Plan 3 returns focus to the window after every click, so a volunteer who starts typing a song title without clicking the search box would otherwise move the room screen with no sign on the laptop. A non-song live item ignores the keys with the toast "Section keys work on songs." and the room never changes.

**Which occurrence (the "jump rule"):** for a key that matches kind K (and verse number n for digits):

1. If the live slide is inside an occurrence of K, go to the **first slide of that same occurrence** (the leader went "back to the top of the chorus").
2. Otherwise go to the first occurrence of K **after** the live slide.
3. Otherwise go to the **last** occurrence of K before the live slide (the leader repeats a chorus after the last written one).
4. If the song has no K: nothing changes on screen; toast "This song has no Chorus." (the kind's display name).

**Unnumbered verses.** If no Verse section in the song carries a number (hymns imported with plain "Verse" headings), digit n means the nth distinct Verse section in written order; the jump rule then applies to the occurrences of that section.

Step by step, typical Sunday: the song is live on Verse 2. The leader goes back into the chorus. The operator presses `C`. The screen shows the first chorus slide after Verse 2 and the toast says "Jumped to Chorus (C)". Next continues from there through the arrangement as usual. After the last chorus the leader sings it once more: operator presses `C` again while on the last chorus's last slide → rule 1 → top of that chorus.

If nothing is live: toast "Nothing is live yet." The room never changes. Help (Plan 3) gains the rows "1–9 — Verse 1–9 of the live song" and "C P B T I E L V — Chorus, Pre-chorus, Bridge, Tag, Intro, Ending, Interlude, next Verse".

### 3.3 Jumping from the phone

The phone shows the live song's section chips (§3.1). Tapping one sends a Jump for that exact occurrence (`occurrence` + its `kind`, §5.2) with the phone's `base` version. No confirm, same as Next. If the screen moved since the phone last updated, the core rejects it as stale and the phone resyncs (existing behaviour in `web/remote.js`), and toasts "The screen changed — try again." The Settings switch does not affect the phone (chips are deliberate taps, not stray keys).

### 3.4 Fixing words while a song is live

1. The live column has an **Edit words** button (pencil icon) whenever the live item is a song. It opens the song editor (Plan 3 Task 17) with the Lyrics box focused, caret at the first lyric line of the live section.
2. **Whatever opened the editor** (Edit words, the library, the service list, a second service item using the same song), if `song.id == live.song_id` the editor shows a yellow banner: "This song is on screen. Nothing changes on screen until you choose Save and show now or Update screen." and offers **Save and show now**.
3. The operator fixes the typo. The room shows the old words the whole time.
4. The footer has **Cancel**, **Save** and, while the song is live, **Save and show now** (primary).
   - **Save** saves the song. The core reconciles (§5.4): if the slide on screen is unaffected, the live song quietly uses the new words from the next slide on and nothing changes on screen. Otherwise the live column shows a banner (§8) with an **Update screen** button.
   - **Save and show now** saves, then sends `Command::Refresh` for the live item. The screen shows the corrected words at the same place in the song (unless the text is cleared; §5.5).
5. **Update screen** (live column banner) sends the same `Refresh`. The banner disappears when the live item matches the library again (after Refresh, after the edit is undone by hand, or when another item goes live).
6. The banner also says what else shows the new words: "…Update screen shows it now. Clicking a slide also shows the new version." (Clicking a thumbnail sends `Show`, which builds the item from the library, so it applies the edit too.) Next, Back, keys and section chips keep walking the old words until then.
7. The same applies to any other change that reaches the live song: Item edit's order override, a re-import updating the song, a background change, a Settings change to lines per slide, credits or the default background, a backup restore.

### 3.5 Reordering an arrangement

**Song editor (the song's usual order).** The comma text box "Order" is replaced by the arrangement strip:

```
Order   [V1] [C] [V2] [C] [B] [C]  ⟲ Use written order
Add:    + Verse 1  + Chorus  + Verse 2  + Bridge  + Ending
```

- Drag a chip left/right and drop it on another chip: it takes that chip's place (the others shift), the same drop rule as the service list (Plan 3 Task 6). A drop zone after the last chip moves it to the end. The dragged chip is shown at 40 % opacity; the drop target shows a 2 px accent bar on its left edge.
- `+ <label>` buttons (one per section in the lyrics) append that section. Repeats are allowed.
- Hover a chip → a small × removes that occurrence.
- Keyboard: click (or Tab to) a chip to select it; `Ctrl+←`/`Ctrl+→` move it, `Delete`/`Backspace` removes it. Without Ctrl, `←`/`→` move the selection.
- **Use written order** empties the arrangement (the song plays in the order the lyrics are written) and is disabled when it is already empty. While empty, the strip shows the written order as faded chips with the hint "Written order — drag to change", and the first drag or `+` turns it into a real arrangement.
- An entry that matches no section in the lyrics shows as a grey dashed chip with tooltip "Not in the lyrics — it is skipped". (This replaces the Plan 3 "Not in the lyrics: X, Y" line.)
- The slide preview to the right updates 250 ms after each change (Plan 3 behaviour).

**Item edit (this service only; build step d).** For a song item, Item edit gets a section "Order for this service" with the same strip and a checkbox **Use the song's usual order** (checked by default). Unchecking copies the song's usual order (or written order) into the strip for editing. **Save** stores the override on the service item only, and the item shows the "Custom order" badge. A link-style button **Make this the song's usual order** saves the strip into the song (`song/save`), clears the override, and toasts "Saved as the usual order."

**Copies.** "Copy last week" and "New from template" (FEAT-6) **drop** overrides: next week starts from the song's usual order. "Copy this service" (an exact duplicate, e.g. for a second service the same day) keeps them.

## 4. Data model

### 4.1 New and changed types

New module `src/sections.rs` (core, no UI types):

```rust
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind { Verse, Chorus, PreChorus, Bridge, Tag, Intro, Ending, Interlude, Other }

impl SectionKind {
    pub fn from_label(label: &str) -> (SectionKind, Option<u8>); // "Chorus 2" → (Chorus, Some(2))
    pub fn short(self, number: Option<u8>) -> String;        // "V2", "C", "PC", "B", "T", "I", "E", "Int", "•"
    pub fn color(self) -> &'static str;                        // canonical palette, §6.1
    pub fn key(self) -> Option<char>;                          // 'c','p','b','t','i','e','l'; Verse → 'v'; Other → None
    pub fn name(self) -> &'static str;                         // from src/strings/song.rs: "Chorus", "Pre-chorus"…
}
/// Convenience for FEAT-3 and the phone: colour for a slide label, None for `Other`.
pub fn color_for_label(label: &str) -> Option<&'static str>;

/// Where a song slide came from. Built, never stored in data.json.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SectionRef {
    pub label: String,        // as written: "Chorus", "Verse 2"
    pub kind: SectionKind,
    pub number: Option<u8>,   // Verse 2 → 2
    pub verse_nth: Option<u8>,// nth distinct Verse section in written order (1-based), for unnumbered verses
    pub occurrence: u16,      // index in the played order (0-based)
    pub ordinal: u16,         // nth time this label is played (0 = first chorus, 1 = second…)
    pub page: u16,            // slide index inside this occurrence
    pub pages: u16,
}

pub struct JumpTarget { pub kind: SectionKind, pub number: Option<u8>, pub occurrence: Option<u16> }
pub enum JumpError { NotASong, NoSuchSection(SectionKind, Option<u8>), NoSuchOccurrence, KindMismatch }

pub fn jump_target(slides: &[Slide], current: usize, t: &JumpTarget) -> Result<usize, JumpError>;
/// True when two slides look the same in the room: compares `content` only
/// (text, footer, reference, asset). Labels, notes and `section` are ignored.
pub fn same_on_screen(a: &Slide, b: &Slide) -> bool;
pub fn map_position(old: &[Slide], old_index: usize, new: &[Slide]) -> Mapped; // §5.4
pub enum Mapped { Exact(usize), Moved(usize), Lost }
pub fn occurrences(slides: &[Slide]) -> Vec<Occurrence>; // section bar and phone chips
pub struct Occurrence { pub occurrence: u16, pub label: String, pub kind: SectionKind, pub number: Option<u8>, pub first_slide: usize, pub slides: usize }
pub fn heading_offset(lyrics: &str, label: &str, nth: usize) -> Option<usize>;
```

`from_label` rules (lower-cased first word, same word list as `slides::section_label`): verse→Verse; chorus, refrain→Chorus; pre-chorus, prechorus, "pre chorus"→PreChorus; bridge→Bridge; tag→Tag; intro→Intro; ending, end, outro, coda→Ending; interlude, instrumental, turnaround, vamp→Interlude; post-chorus, misc, "Part N" (unlabelled songs), and any bracketed custom label → Other. The number is the first all-digit suffix word (≤ 2 digits, 1–99); letters like `2a` give no number.

`model.rs` changes (all with serde defaults; old `data.json` loads unchanged):

```rust
pub struct Slide {
    pub label: String,
    pub content: SlideContent,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<SectionRef>,          // NEW. Some for song slides only.
}
impl Slide {
    pub fn new(label: impl Into<String>, content: SlideContent) -> Self; // notes empty, section None
}

pub enum ItemKind {
    Song {
        song_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arrangement: Option<Vec<String>>,     // NEW. None = use the song's arrangement.
    },
    // others unchanged
}

pub struct Settings {
    // existing fields…
    #[serde(default = "yes")]
    pub section_keys: bool,                   // NEW. Settings → Presenting → Section keys.
}
```

Every existing `Slide { label, content, notes }` struct literal (`slides.rs` builders, `bible.rs`, `live.rs` and `room.rs` tests, `app.rs` tests) becomes `Slide::new(label, content)` (plus `notes` set where it was non-empty), so later fields never break them again. Every `ItemKind::Song { song_id }` pattern gains `..` (`app.rs` song/delete check, `slides::build_item`; the web operator JSON is unaffected because the field is optional).

`Song.arrangement: Vec<String>` stays as it is (labels, repeats allowed, empty = written order). No `Data.format` bump. An older build still reads files written by this one, but it **drops** `ItemKind::Song.arrangement` (and `section_keys`) the next time it saves: a downgrade loses per-service orders. That is accepted (§11 D17) because downgrades are rare and the song's usual order is untouched.

### 4.2 Live state

`src/live.rs`:

```rust
pub enum Command {
    // existing variants…
    /// Jump the live song to a section. Relative: needs `base`.
    /// With `occurrence`, `kind` must equal that occurrence's kind.
    Jump { item_id: Uuid, kind: SectionKind, #[serde(default)] number: Option<u8>, #[serde(default)] occurrence: Option<u16> },
    /// Replace the live snapshot with the library version and keep the place in it. Operator only.
    Refresh { item_id: Uuid },
}
impl Command { fn relative(&self) -> bool { matches!(self, Command::Next | Command::Prev | Command::Jump { .. }) } }

pub struct LiveItem {
    // existing: item_id, title, kind, slides, background
    pub song_id: Option<Uuid>,                // NEW. Some for song items; never changes for an item.
}

pub struct Live {
    // existing fields…
    /// The live item no longer matches the library and the room would look different.
    pub changed: Option<LiveChange>,
}
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LiveChange { pub item_id: Uuid, pub reason: ChangeReason }
#[derive(Serialize, Clone, Copy, Debug, PartialEq)] #[serde(rename_all = "snake_case")]
pub enum ChangeReason { WordsOnScreen, LookOnScreen, PlaceLost, NoSlides }
```

`Snapshot` (the revert history) also stores `changed`, so a revert after a failed prepare restores the matching banner.

`app.rs` `LiveView` gains (and `LiveItemSummary` gains `song_id`):

```rust
pub sections: Vec<OccurrenceView>,   // ALWAYS sections::occurrences(&live.item.slides): the snapshot, never the library
pub changed: Option<LiveChange>,
// OccurrenceView { occurrence, label, short, kind, color, first_slide, slides, live: bool }
```

`Slide.section` flows through `LiveView.slide` and `NextUp.slide`, so the stage and phone get section labels for current and next with no extra field.

## 5. Core behaviour

### 5.1 Building song slides

`slides::song_order` changes to `song_order(order: &[String], sections: &[Section]) -> Vec<usize>` and `song_slides(song, order: &[String], settings)`. New `slides::item_order<'a>(item: &'a ServiceItem, song: &'a Song) -> &'a [String]` returns `item.arrangement.as_deref().unwrap_or(&song.arrangement)`; `build_item` and FEAT-6 print both use it, so print follows the per-service order. While building, each page gets its `SectionRef`: `occurrence` counts entries of the played order that matched a section; `ordinal` counts earlier occurrences with the same label (case-insensitive); `verse_nth` numbers distinct Verse sections in written order; `page`/`pages` count across all of the section's blank-line groups (so "Bridge (2/3)" labels and `page` agree). Slide labels do not change from today ("Verse 1 (1/2)").

Unknown arrangement entries are skipped, as today. If every entry is unknown, the written order is used, as today.

### 5.2 Jump

`Live::run` for `Command::Jump { item_id, kind, number, occurrence }`:

1. Base check as for Next (required; stale → "stale: the screen changed before this arrived").
2. No cursor → `Err("Nothing is live yet.")`. Cursor item ≠ `item_id` → `Err("The live song changed — try again.")` (protects a phone tap that raced a Take Live; the base check normally catches it first).
3. Resolve against the **live snapshot** (`self.item.slides`), the same slides Next walks (v1 §7.2). If no slide has `section` → `Err("Section keys work on songs.")`.
4. If `occurrence` is `Some(o)` (phone chip, or a section-bar click on the live item): target = first slide of occurrence `o`; no such occurrence → `Err("The song changed — try again.")`; occurrence `o` has a kind other than `kind` → the same error (the phone's chip list was out of date). Otherwise `sections::jump_target` applies the jump rule in §3.2. `V` sends `kind: Verse, number: None`; a digit sends `kind: Verse, number: Some(d)`; digit matching uses `number`, or `verse_nth` when no verse in the song has a number.
5. `self.go(item, cursor.item_index, target)`: the normal slide transition, so the clear matrix and layer rules (v1 §6.3) hold: a jump after Clear Slide, Clear All or Logo brings the text and background back, exactly like Next. A jump is a navigation action, which v1 allows to undo a clear.

A jump to the slide that is already live still calls `go` (new version; the room diff sees an identical frame and redraws nothing). Errors from Jump never change the frame or version. The operator glue shows the success toast from the outcome (§3.2); the core knows nothing about toasts.

### 5.3 Phone allowlist

`server.rs remote_command`: add `Command::Jump { .. }` to `allowed`, and require `env.base.is_some()` for it (the core also requires it). `Command::Refresh` is **not** allowed from the phone. The v1 §12.3 allowlist line becomes: "`Next`, `Prev`, `GoToCue`, `Jump` (section of the live song), `TakeLive`…". The phone's chips always send `occurrence` and its kind, never a bare kind, so a tap means exactly the chip it hit.

### 5.4 Reconciling the live item

New `Live::reconcile(&mut self, plan: &dyn Plan) -> bool` (returns whether live state changed). Lock order is always data → live (the same as `live_view`). It runs:

1. in `App::edit` after the mutation, **whether or not the save succeeded** (memory has already changed), before the error is returned;
2. in `backup/restore` after `*app.data` is replaced;
3. in `App::room_event` right after `Live::report` reverts to a committed snapshot on `RoomEvent::Failed` (the reverted snapshot may predate a quiet adoption, so it must be re-checked against the library). `room_event` therefore takes the data lock before the live lock.

Each caller broadcasts live afterwards when it returns true.

Algorithm, with `old = self.item` (the snapshot) and `new = plan.build(old.item_id)`:

1. Nothing live → return false. `new` is `None` (item removed from the service, or `service/select` switched services): clear `changed` if it named this item and return whether it was set. The v1 removed-item rule for navigation continues.
2. `new.slides == old.slides` (full `PartialEq`) and backgrounds equal → clear `changed` if set; done.
3. `new.slides` empty → `changed = NoSlides`. Keep the snapshot.
4. `m = sections::map_position(&old.slides, cursor.slide, &new.slides)`.
   - `Exact(i)` or `Moved(i)`, **and** `same_on_screen(&new.slides[i], &old.slides[cursor.slide])`, **and** backgrounds equal → **quiet adoption** (§5.7): `self.item.slides = new.slides` (only `slides`; every other snapshot field, including FEAT-2 `advance` and any FEAT-1 theme, is kept), `cursor.slide = i`, `changed = None`, remap FEAT-2 auto-advance (below), `bump()`. The frame is not touched. The bump makes any in-flight Next/Jump stale, so nobody advances through slides they did not see.
   - `Lost` → `changed = PlaceLost`.
   - Otherwise pick the reason, first match wins: (a) the live slide's text and reference differ **and** the song's words are not the same when every slide's text is joined and whitespace is collapsed → `WordsOnScreen`; (b) anything else (same words split differently by lines per slide, footer/credit/CCLI line, background) → `LookOnScreen`.
   - In both banner cases the snapshot, frame and version are unchanged; Next/Prev/Jump keep walking the old slides until Refresh or a Show.

**FEAT-2 auto-advance during adoption.** If an auto-advance run is active for this item, adoption keeps its due time, rewrites any stored `from_slide` from the old to the new cursor index, and leaves `waiting_for` alone; FEAT-2 treats "committed ≥ waiting_for" as satisfied, so the adoption's own commit cannot stall it. A pending `AutoNext { from_slide: old }` that arrives after adoption is matched against the remapped index (the core keeps `last_adopt: Option<(old, new)>` for the current version and translates `from_slide` once).

**Room after adoption.** The new version has an identical frame; `room.rs` compares layer keys (built from `SlideLayer`, not from `Slide`, so labels and `section` never reach them), prepares nothing, and reports `Committed`; `pending()` becomes false.

`map_position` (pure, in `sections.rs`):

1. Old slide has no `section` (not a song): match by index if `new.len() > index` and `same_on_screen` → `Exact`; else `Lost`.
2. Build the occurrence label lists (lower-cased) of old and new played orders. Align them with a longest-common-subsequence diff, backtracking twice (preferring earliest matches, then latest matches). If both alignments pair the live occurrence with the same new occurrence `q`: same page in `q` → `Exact(i)`; `q` has fewer pages → its last page → `Moved(i)`. This keeps the physical occurrence when an earlier repeat is removed or a new one inserted.
3. Not aligned or ambiguous (the two backtracks disagree): same `(label, ordinal)` in new → same page clamped → `Moved(i)`.
4. Label renamed: same `occurrence` index whose kind and number equal the old one's → same page clamped → `Moved(i)`.
5. Otherwise `Lost`.

Example: order `C V1 C V2 C`, live on the second chorus. Removing the first `C` gives `V1 C V2 C`; the alignment pairs old occurrence 2 with new occurrence 1 (the chorus between V1 and V2), so the next Next still goes to V2.

### 5.5 Refresh

`Command::Refresh { item_id }` (no base; operator only; an explicit operator action, equivalent to re-taking the item at the same place):

1. Live cursor item ≠ `item_id` → `Err("That song is no longer live.")`.
2. `new = plan.build(item_id)`; missing or no slides → `Err("That song has no slides now. The screen keeps the old words.")`; `changed` stays.
3. Position = `map_position(...)`; `Lost` → slide 0 (v1 §7.2 "falls back to slide 1").
4. Slide layer **up** (`frame.slide.is_some()`): `self.go(new, cursor.item_index, position)`; `changed = None`. The room shows the new words with the normal transition. Same background → keeps playing (existing `set_background` rule).
5. Slide layer **down** (Clear Slide, Clear All or Logo is in effect): replace the whole snapshot with `new`, set `cursor.slide = position`, `changed = None`, `bump()`, and **leave the frame alone**. A clear is only undone by a show or navigation action; the next Next, Jump or click shows the new words. The toast "Saved. The new words show when you bring the text back." tells the operator why nothing moved.
6. A prepare failure reverts to the committed snapshot (v1 §5.2), then `room_event` reconciles (§5.4), which puts the banner back.

Refresh replaces the whole snapshot (not only `slides`), because it is an explicit operator choice like Take Live: a new FEAT-2 interval or FEAT-1 theme in the item takes effect.

"Save and show now" = `song/save`, then `Refresh` if the save succeeded. If `song/save` fails, no Refresh is sent and the editor stays open with the core's error toast.

### 5.6 Interaction with the rest of live

- **Clear matrix:** reconciliation never touches `frame`; Refresh with the slide layer down does not either (§5.5). Jump is a navigation action and behaves exactly like Next.
- **Logo / Clear All while banner up:** banner stays (it is about the snapshot, not the frame). Next after Clear All walks the snapshot as today.
- **Take Live / Show of any item:** `go` replaces the snapshot; `changed` is cleared in `go`. Show of the live item's own thumbnail therefore applies the edit (§3.4 step 6).
- **Removing the live song from the service:** reconcile step 1 clears the banner (no Refresh possible). v1 rule continues.
- **Two quick saves:** each reconciles; the second sees the snapshot from the first adoption. Idempotent.
- **Duplicate command ids:** handled by the existing 1,024-id memory; a re-delivered Jump does nothing.
- **Foldback/phone "next":** `next_preview` reads the snapshot, so after quiet adoption the next cue shows the corrected words at once.

### 5.7 Amends v1 §7.2 ("Live item edited in library")

v1 says live navigation uses the snapshot taken at Take Live until the operator re-takes it. The row now reads:

> Live navigation uses the snapshot taken at Take Live. After any data change the core compares the snapshot with the library. If the slide on screen and the background would look exactly the same (`same_on_screen`), the core copies **only the new `slides`** into the snapshot and moves the cursor to the same place, without changing the frame (quiet adoption). No other snapshot field (title, background, FEAT-2 `advance`, FEAT-1 theme) is copied. Otherwise the snapshot is kept and the operator sees a banner; only `Refresh`, Take Live or a Show applies the change.

Consequences for other specs (to be updated by their owners; recorded in §10): FEAT-3's line "Live song editing changes the snapshot only through FEAT-4's explicit update" becomes "…through FEAT-4's explicit update, or FEAT-4 quiet adoption of slides when the screen is unaffected". FEAT-2 §5.4 "editing the item's interval while it's live takes effect the next time it's taken live" stays true (adoption never copies `advance`), with "or Update screen" added.

## 6. Rendering

- The room output and the compositor (`src/render/`) do not change. Section colours never reach the room. Room layer keys are built from `SlideLayer` (text, footer, reference), so `Slide.section`, labels and notes cannot cause a redraw.
- **Theme today:** `LiveView.theme` is read from `data.settings.theme` and the room re-plans as soon as it changes (`room.rs` compares `last.theme`). The theme is outside the snapshot and outside reconcile, and a theme edit reaches the room immediately. FEAT-4 does not change that; FEAT-1 owns the rule (§10).
- Thumbnails: the colour band and chip are Slint elements over the thumbnail image, so `ThumbKey` (Plan 3 Task 7) is unchanged and thumbnails are not re-rendered when only the arrangement changes.
- Stage (`ui/stage.slint`, until FEAT-3 replaces the layout): a label line above the current text, text = `SectionRef.label` (plus " (2/3)" when `pages > 1`), colour = `SectionKind::color()` passed in as a `color` property from Rust (stage files keep their literals per Plan 3, so the colour comes from data, not `Theme`).
- Phone (`web/remote.js`, `remote.html`): chip row from `LiveView.sections`; colours come from `OccurrenceView.color`, so the web page has no palette of its own.

### 6.1 Palette (canonical in `SectionKind::color`)

| Kind | Colour | Short | Reason |
| --- | --- | --- | --- |
| Verse | `#4c8dff` blue | V1, V2… | Common convention in EasyWorship/ProPresenter |
| Chorus | `#d65db1` magenta | C | Not red: red means "live" in this app |
| Pre-chorus | `#2bb3a8` teal | PC | |
| Bridge | `#8b6cf0` violet | B | |
| Tag | `#f08a3c` orange | T | |
| Intro | `#7fa7c9` slate blue | I | |
| Ending | `#7a8599` grey-blue | E | |
| Interlude | `#b5a254` olive | Int | |
| Other | `#5d6576` grey | • | Same as `Theme.faint` |

All have at least 3:1 contrast against `Theme.panel` (#15181e) for the 4 px band, except Other, which is meant to recede; chip text is `Theme.text` on the colour at 22 % alpha with a 1 px solid border in the colour, so text contrast does not depend on the hue. The palette test (§9.1) enforces distinctness; if a colour fails it, it is adjusted in lightness before release.

## 7. UI (extends Plan 3)

All `.slint` colours come from `Theme`; no hex literals in operator/dialog files (Plan 3 rule).

**`ui/theme.slint`**: add `out property <color> sec-verse, sec-chorus, sec-prechorus, sec-bridge, sec-tag, sec-intro, sec-ending, sec-interlude, sec-other` with the §6.1 values, and a pure function `section-color(kind: string) -> color`.

**`ui/widgets.slint`**: new `SectionChip { in property <string> short; in property <string> label; in property <string> kind; in property <bool> live; in property <bool> compact; in property <string> tooltip; callback clicked(); }`.

**New `ui/operator/section-bar.slint`**: `SectionBar { in property <[SectionBarChip]> chips; callback pick(int /*occurrence*/); }`, `export struct SectionBarChip { occurrence: int, short: string, label: string, kind: string, live: bool, tooltip: string }`. Horizontal `Flickable` when it overflows. `accessible-id: "section-bar"`, chips `sec-<n>`.

**`ui/operator/slides.slint`** (Plan 3 Task 7): `SlideThumb` gains `section-kind: string`, `section-short: string`; draws the band and the chip. `SectionBar` sits between the header and the grid, hidden when `chips` is empty. The header shows "Custom order for this service" when the item has an override.

**`ui/operator/services.slint`** (Plan 3 Task 6): the row gains `in property <bool> custom-order` → a `Chip` "Custom order" (`accessible-id: "custom-order-<n>"`).

**`ui/operator/live.slint`** (Plan 3 Task 9): section line under the preview; **Edit words** `Btn small` (visible when the live kind is song); the change banner (`Theme.notice-bg`/`notice-border`, text by `ChangeReason` from strings) with **Update screen** `Btn primary small` (hidden for `NoSlides`). Accessible ids `live-edit-words`, `live-changed`, `live-update-screen`.

**`ui/dialogs/settings.slint`** (Plan 3): a `CheckBox` "Section keys (C, B, 1–9…)" in Presenting, `accessible-id: "settings-section-keys"`, with the hint "Turn off if letters typed by mistake move the screen."

**New `ui/dialogs/arrangement.slint`**: `ArrangementStrip { in property <[ArrChip]> chips; in property <[ArrAdd]> adds; in property <bool> written-order; in-out property <int> selected; callback move(int from, int to); callback remove(int); callback add(string label); callback reset(); }`. Drag uses `DragArea` with data `arr:<index>` and a `DropArea` per chip accepting only `arr:` data from another index (the Plan 3 service-list pattern), plus an end drop zone (`to = len`). Its own `FocusScope` handles Ctrl+←/→, ←/→ and Delete/Backspace while a chip is selected. `ArrChip { label: string, short: string, kind: string, missing: bool, faded: bool }`.

**`ui/dialogs/song-editor.slint`** (Plan 3 Task 17): replace the Order `LineEdit` and the `+ label` chips with `ArrangementStrip`; add `in property <bool> live-song` (banner + **Save and show now** button); `callback save-and-show()`.

**`ui/dialogs/item-edit.slint`** (Plan 3 Task 18): for kind song, an "Order for this service" `Field` with a `CheckBox` "Use the song's usual order" and the `ArrangementStrip` (disabled while checked), plus the **Make this the song's usual order** link button.

**Rust glue:**

- `src/ui/operator/keys.rs`: `KeyAction::Jump { kind: SectionKind, number: Option<u8> }`. `route` adds a step after Enter: a single character with no Ctrl/Cmd/Alt, not in a text field, no dialog, `settings.section_keys` true (passed in `KeyCtx.section_keys`) → digits `1`–`9` → `Jump{Verse, Some(d)}`; letters per `SectionKind::key` (case-insensitive) → `Jump{kind, None}`; `0` and other letters → `None`. The handler: nothing live → toast "Nothing is live yet." without sending; live item not a song (`LiveView.sections` empty) → toast "Section keys work on songs." without sending; else `Ops::cmd(Command::Jump { item_id, kind, number, occurrence: None })` with `base = live.version`, and on success toasts "Jumped to <Name> (<key>)".
- New `src/ui/operator/sections.rs`: `pub fn bar_chips(snap, selected: Option<Uuid>, live: &LiveView, keys_on: bool) -> Vec<SectionBarChip>`: when `selected == live item`, from `live.sections` with the live mark and key hints; otherwise from the selected item's library build (`slides::build_item`, off the UI thread in `OperatorSnapshot`) with no hints. `pick(o)`: live item → `Jump { occurrence: Some(o), kind }` with `base = live.version` (a stale result toasts "The screen changed — try again."); other item → `Show { item_id, slide: first_slide(o) }` (operator Show, no base, Plan 3 rule).
- `src/ui/operator/live.rs`: Edit words opens `DialogKind::SongEditor { song_id, add_after: false, caret_section: Some(occurrence) }`; Update screen sends `Command::Refresh`.
- New `src/ui/dialogs/arrangement.rs` (pure, unit-tested): `move_entry(&mut Vec<String>, from, to)`, `remove(&mut Vec<String>, i)`, `chips(lyrics: &str, arrangement: &[String]) -> (Vec<ArrChipData>, Vec<String> /*adds*/, bool /*written*/)`, `materialise_written(lyrics) -> Vec<String>`.
- `src/ui/dialogs/song_editor.rs`: `live-song = (song.id == live.song_id)`, recomputed on every live update while open; caret placement with `TextEdit.set-selection-offsets(o, o)` at `sections::heading_offset(lyrics, label, nth)`; "Save and show now" = `song/save` then `Ops::cmd(Refresh)`; `order_warnings` is replaced by `ArrChip.missing`.
- `src/ui/dialogs/item_edit.rs`: override on/off and strip; save goes through the existing `service/update` (the full `ServiceItem`, now carrying `arrangement`). "Make this the song's usual order" = `song/save` with the strip, then `service/update` with `arrangement: None`.

**Strings.** Shared convention for all roadmap specs: core and operator strings for an area live in `src/strings/<area>.rs` as `pub const` items, with `src/strings/mod.rs` declaring the modules. FEAT-4 uses `src/strings/song.rs` (it creates `mod.rs` if no other FEAT has yet). The core's new error texts are taken from there too. The phone's strings sit in one `const STR = {…}` object at the top of `web/remote.js`. Wording follows `web/operator.js` tone: short, plain, no jargon.

## 8. Errors

| Situation | What the operator sees | Room |
| --- | --- | --- |
| Jump key, nothing live | Toast "Nothing is live yet." | Unchanged |
| Jump key, live item not a song | Toast "Section keys work on songs." | Unchanged |
| Jump, section not in song | Toast "This song has no Bridge." / "This song has no Verse 4." | Unchanged |
| Jump with occurrence out of date (phone or chip) | "The song changed — try again." | Unchanged |
| Jump/Next stale (phone or a raced key) | Phone: "The screen changed — try again." Operator key: none (Plan 3 `outcome_toast` hides stale for keys); chip click: same toast as phone | Unchanged |
| Save fails (disk) | Error toast with the core message; editor stays open; reconcile still runs on the changed memory | Unchanged |
| Words on the slide on screen changed | Banner "You changed the words that are on screen. Update screen shows them now. Clicking a slide also shows the new version." + Update screen | Unchanged until Update screen |
| Same words, different look (split, footer, credits, background) | Banner "The live song looks different now. Update screen to show it." + Update screen | Unchanged until Update screen |
| Place in song lost (section removed) | Banner "The part of the song on screen was removed. Update screen starts the song from the top." + Update screen | Unchanged until Update screen |
| Song now has no slides | Banner "This song has no words now. The screen keeps the old words." No Update button | Unchanged |
| Refresh but song no longer live | Toast "That song is no longer live." | Unchanged |
| Refresh while text is cleared | Toast "Saved. The new words show when you bring the text back." | Clear stays |
| Refresh prepare fails in the room | Existing notice "Could not show that: …"; banner returns via reconcile after revert | Old words stay |
| Arrangement entry missing from lyrics | Grey dashed chip, tooltip "Not in the lyrics — it is skipped" | n/a |
| Override references a renamed section | Same dashed chip in Item edit | Skipped entry, as today |

## 9. Testing

### 9.1 Core unit tests

`src/sections.rs`:
- `from_label_kinds_and_numbers` (`Verse 1`→(Verse,1), `Chorus 2`→(Chorus,2), `Pre-Chorus`→PreChorus, `pre chorus`→PreChorus, `Refrain`→Chorus, `Outro`/`End`/`Coda`→Ending, `Vamp`→Interlude, `Part 3`→Other, `Spontaneous`→Other, `Verse 2a`→(Verse,None)).
- `short_names` (V2, C, PC, B, Int, •), `color_for_label_matches_kind_color`.
- `palette_is_distinct_and_not_live_red`: every pair of the eight non-Other colours has CIEDE2000 ΔE ≥ 15; each is ≥ 25 from `Theme.live` `#ff4f55`.
- `jump_restarts_current_occurrence`, `jump_goes_to_next_occurrence_ahead`, `jump_falls_back_to_last_before`, `jump_digit_selects_verse_number`, `digit_falls_back_to_nth_unnumbered_verse`, `jump_v_goes_to_next_verse_any_number`, `jump_explicit_occurrence`, `jump_occurrence_kind_mismatch_errors`, `jump_missing_kind_errors`.
- `same_on_screen_ignores_label`, `same_on_screen_ignores_notes`, `same_on_screen_ignores_occurrence_shift`, `same_on_screen_sees_footer_change`.
- `map_exact_same_label_ordinal_page`, `map_moved_when_pages_shrink`, `map_keeps_physical_occurrence_when_earlier_repeat_removed` (C V1 C V2 C, live on 2nd C, remove 1st C → new occurrence 1), `map_keeps_physical_occurrence_when_chorus_inserted_before`, `map_ambiguous_alignment_falls_back_to_ordinal`, `map_moved_when_label_renamed`, `map_lost_when_section_deleted`, `map_non_song_by_index`.
- `heading_offset_finds_nth_heading`.

`src/slides.rs`:
- `song_slides_tag_section_ref` (occurrence, ordinal, page, pages for `V1 C V2 C` with a two-page chorus).
- `repeated_chorus_gets_distinct_ordinals`, `unnumbered_verses_get_verse_nth`.
- `item_order_prefers_item_override`, `empty_override_list_means_written_order` (`Some(vec![])` = written order, same as the song rule).
- `slide_labels_unchanged` (regression: labels identical to before FEAT-4).

`src/live.rs`:
- `jump_requires_base`, `jump_queued_before_clear_all_is_stale`, `jump_after_clear_slide_brings_text_back`, `jump_on_non_song_errors_and_keeps_version`, `jump_wrong_item_id_errors`, `duplicate_jump_id_applies_once`.
- `edit_other_section_adopts_quietly_frame_unchanged` (frame equal, version +1, `next_preview` shows the new words).
- `adoption_copies_only_slides` (snapshot title, background and an `advance` stub stay as taken).
- `label_only_change_adopts_quietly`, `notes_only_change_adopts_quietly`, `occurrence_shift_adopts_quietly`.
- `edit_live_slide_sets_words_on_screen_and_keeps_frame_and_version`.
- `lines_per_slide_change_gives_look_reason`, `footer_change_gives_look_reason`, `background_change_gives_look_reason`.
- `arrangement_change_while_live_keeps_position` (insert a chorus before the live verse → cursor index moves, frame unchanged, `changed` None).
- `refresh_shows_new_words_at_same_position`, `refresh_lost_place_goes_to_slide_zero`, `refresh_with_no_slides_errors_and_keeps_changed`, `refresh_wrong_item_errors`.
- `refresh_while_cleared_keeps_clear` (Clear Slide, then Refresh → frame unchanged, `changed` None, version +1; Next shows the new words), `refresh_while_logo_keeps_logo`.
- `failed_refresh_prepare_reverts_snapshot_and_banner`.
- `failed_prepare_after_adoption_rechecks_library` (adopt, then a Failed for a later version reverts; `room_event` reconcile adopts again or sets the banner, never leaves a silent mismatch).
- `quiet_adoption_keeps_auto_advance_running` (due time unchanged; an `AutoNext` with the old `from_slide` advances from the new index).
- `unchanged_frame_commit_clears_pending` (after adoption, `report(Committed{new version})` → `pending()` false).
- `take_live_other_item_clears_changed`, `removed_live_item_has_no_banner`, `service_switch_clears_banner`.

`src/room.rs`:
- `unchanged_frame_new_version_commits_without_prepare` (same `SlideLayer`, new version → no prepare request, `Committed` reported).

`src/model.rs` / `src/store.rs`:
- `old_data_without_item_arrangement_loads` (a `data.json` fixture from `native-foundation`), `old_settings_default_section_keys_on`.
- `item_arrangement_none_is_not_serialized`.

`src/server.rs`:
- `phone_jump_with_base_allowed`, `phone_jump_without_base_rejected`, `phone_refresh_rejected`.

`src/app.rs`:
- `edit_reconciles_live_under_data_then_live_lock` (save a song while live → `LiveView.changed` set, no deadlock with a concurrent `live_view`).
- `edit_reconciles_even_when_save_fails` (store made read-only).
- `restore_while_live_reconciles`.
- `live_view_lists_sections_with_live_flag`, `phone_chips_follow_snapshot_while_changed` (banner up → `LiveView.sections` equals the snapshot's occurrences, not the library's).
- `settings_lines_per_slide_while_live_gives_look_banner`.

### 9.2 UI tests (Slint testing backend, `Ops::inline`)

`src/ui/operator/keys.rs`: `letters_and_digits_route_to_jump`, `l_routes_to_interlude`, `jump_keys_ignored_in_text_field`, `jump_keys_ignored_with_dialog_open`, `jump_keys_off_in_settings_do_nothing`, `ctrl_or_alt_letter_is_not_a_jump`, `zero_and_unmapped_letters_do_nothing`.
`src/ui/operator/sections.rs`: `bar_chips_follow_arrangement`, `bar_marks_live_occurrence_only_for_live_item`, `key_hints_only_on_live_item`, `chip_click_on_live_item_sends_jump_occurrence`, `chip_click_on_other_item_shows_first_slide`.
Window-level (`tests/ui_operator_flows.rs`): `c_key_jumps_live_song_to_chorus_and_toasts`, `stray_typing_with_no_field_focused_toasts_each_jump` (type "c2" with a song live → two jumps, two toasts naming them), `stray_typing_on_non_song_live_item_changes_nothing`, `c_key_with_preview_on_other_song_still_acts_on_live`, `thumb_band_kind_matches_slide_section`, `theme_section_tokens_match_rust_palette`, `edit_words_opens_editor_at_live_section`, `editor_from_library_shows_live_banner_for_live_song`, `save_changes_words_on_screen_shows_banner`, `update_screen_clears_banner_and_shows_new_words`, `save_and_show_now_updates_live`, `save_unrelated_section_shows_no_banner`, `custom_order_badge_shows_on_service_row`.
`src/ui/dialogs/arrangement.rs`: `move_entry_left_and_right`, `drop_on_self_does_nothing`, `drop_on_end_zone_moves_last`, `remove_entry`, `first_add_materialises_written_order`, `missing_entries_flagged`, `ctrl_arrow_moves_selected_chip`, `delete_key_removes_selected_chip`, `drag_chip_reorders` (via `mock_drag` where it works, otherwise the `move` callback), `reset_to_written_order`.
`src/ui/dialogs/item_edit.rs`: `override_saved_on_item_only`, `use_usual_order_clears_override`, `make_usual_order_saves_song_and_clears_override`.

### 9.3 Manual checks (church laptop, projector, a phone)

1. Live song, press `C`, `2`, `B`, `E`: the room changes in under 200 ms each; a toast names each jump; the stage label follows.
2. Leader repeats the last chorus: `C` on its last slide returns to its top.
3. Phone chips: tap Bridge; tap twice fast after a laptop Next (one stale toast, no double move).
4. Fix a typo in Verse 3 while on Verse 1: nothing flickers; Next to Verse 3 shows the fix.
5. Fix a typo on the slide on screen: banner; room unchanged; Update screen shows the fix on the same slide.
6. Clear Slide, fix a typo, Save and show now: the screen stays clear; Next shows the fix.
7. Background video playing during a lyric refresh keeps playing without a restart.
8. Drag the arrangement with a touchpad and with a mouse on an old laptop; keyboard reorder with Ctrl+arrows.
9. Turn Section keys off; typing letters with no field focused does nothing.
10. (After FEAT-6) Override the order in this service only; Copy last week drops it; the song's usual order is untouched.

## 10. Dependencies on other FEAT specs

- **Plan 3 (required):** operator window, `Ops`, key routing, song editor, Item edit, Settings, service-list drag pattern, `Theme`. FEAT-4 changes those files; it must start after Plan 3 Tasks 6, 7, 9, 10, 17, 18 are merged.
- **Shared key rule (all FEATs):** in the operator's presentation scope (no text field, no dialog), **bare letters A–Z and digits 0–9, with or without Shift, belong to FEAT-4 section jumps** (used now: 1–9, V C P B T I E L; the rest reserved). Other features use Ctrl/Alt combinations or F-keys.
- **FEAT-1 (Looks/themes):** today the theme lives outside the snapshot (`LiveView.theme` from settings; the room re-plans when it changes), so theme edits apply live at once, and FEAT-4's reconcile ignores them. Contract: if FEAT-1 keeps the resolved theme outside `LiveItem`, that stays FEAT-1's rule. If FEAT-1 moves the resolved theme into `LiveItem`, then (a) quiet adoption never copies it (§5.7), (b) reconcile treats a theme difference as `LookOnScreen`, and (c) Refresh applies it. FEAT-1's PROPS/OVERLAY layer is untouched by Jump/Refresh (slide transitions; overlays persist per FEAT-1). Rich text runs in song slides (if FEAT-1 adds them) must be part of `SlideContent` so `same_on_screen` sees them.
- **FEAT-2 (automation):** must not bind bare letters or digits (shared key rule). Macros may not contain `Jump` (it depends on what is live) or `Refresh` (it needs a fresh human look). A Jump restarts the slide timer like any slide change. Quiet adoption keeps an auto-advance run going (§5.4): FEAT-2 must compare `committed ≥ waiting_for` and accept FEAT-4's `from_slide` remap. FEAT-2 §5.4's "takes effect the next time it's taken live" gains "or when the operator chooses Update screen".
- **FEAT-3 (stage):** **must change** its Decision 14 bare **N** (stage note) to **Alt+N** under the shared key rule (right-click stays). The SectionName element calls `sections::color_for_label(label) -> Option<&'static str>` (replacing the `songs::section_color` name in FEAT-3). Its dependency line becomes "Live song editing changes the snapshot only through FEAT-4's explicit update or FEAT-4 quiet adoption (slides only, screen unaffected)". Stage-note edits (`Slide.notes`) adopt quietly (notes are not on the room screen). FEAT-4 provides a minimal label line until FEAT-3 lands.
- **FEAT-5 (Bible):** scripture slides have `section: None`; jump keys toast "Section keys work on songs." Scripture break editing is FEAT-5's and does not use `SectionRef`.
- **FEAT-6 (service planning):** "Copy last week" and templates **drop** `ItemKind::Song.arrangement` (set `None`); "Copy this service" keeps it. Print/export lists each song's played order with short names using `slides::item_order(item, song)` + `slides::song_order(order, sections)` (the new signature), so print follows the override. Practice mode: jump keys and Refresh work unchanged (only the room output is suppressed).
- **FEAT-7 (reporting):** CCLI usage counts a song once per service regardless of jumps or refreshes. Planning Center import (FEAT-7 as written): a new song's valid PCO `sequence` becomes `Song.arrangement`; existing library songs are never modified; FEAT-4 adds nothing and requires nothing more. (A later option to put the PCO sequence into the item override instead is left to FEAT-7 D22.)
- **Strings (all FEATs):** `src/strings/<area>.rs`. FEAT-2 (`src/automation/strings.rs`), FEAT-3 (`src/stage/strings.rs`) and FEAT-6/7 (`text.rs`) should move to this path.

## 11. Decisions for Jake to review

| # | Decision | Default chosen | Main alternative |
| --- | --- | --- | --- |
| D1 | What jump keys act on | The **live** song only | The selected (preview) song, taking it live at that section |
| D2 | Key map | `1`–`9` verse N, `V` next verse, `C` `P` `B` `T` `I` `E` `L` (Interlude) as in §3.2; fixed | Let each church edit the key per section kind in Settings |
| D3 | Which chorus `C` goes to | Restart current chorus if inside one; else next chorus ahead; else last one before | Always the next chorus ahead (pressing `C` twice skips forward) |
| D4 | Section colours | Fixed palette per kind (§6.1); chorus is magenta because red means "live" | Colours editable in Settings |
| D5 | Section colours on the room screen | Never shown to the congregation | Optional small section label for the band (lower corner) |
| D6 | Live edit when the slide on screen is not affected | Quiet adoption of the new slides only (amends v1 §7.2, §5.7); nothing changes on screen | Always show the banner and require Update screen for any change to the live song |
| D7 | Where live fixes are made | The full song editor, opened at the live section | Edit the text of one slide in place on its thumbnail |
| D8 | Save button while live | Two buttons: **Save** and **Save and show now** (primary) | Only Save, then always Update screen |
| D9 | Undo for song edits | None beyond Cancel in the editor | An undo of the last saved edit |
| D10 | Per-service order | Supported (built last, step d): Item edit overrides the order for this service only, with a "Custom order" badge | One order per song only (simpler, but a short version for one week changes it everywhere) |
| D11 | Arrangement editing style | Draggable chips + `+ section` buttons + keyboard; the comma text box is removed | Keep the text box beside the chips for power users |
| D12 | Phone section chips | Shown for the live song, tap jumps, no confirm (like Next) | Hidden behind a "Sections" button, or confirm before jumping |
| D13 | After Lost place on Update screen | Start the song from slide 1 (v1 §7.2) | Stay on the nearest remaining section |
| D14 | Unlabelled songs ("Part 1", "Part 2") | Grey "Other" chips, no jump key | Treat each part as a verse (1–9 jump to part N) |
| D15 | Accidental typing safety | Section keys on by default, a toast after every key jump, and a Settings switch to turn them off | Keys off by default (churches opt in) |
| D16 | Copying a service with a custom order | Copy last week and templates drop it; Copy this service keeps it | Always keep it (the override follows the item forever) |
| D17 | Downgrade to an older build | Accepted: an older build that saves drops per-service orders | Bump `Data.format` so older builds refuse to open the file |
| D18 | Update screen while the text is cleared | Takes the new words but keeps the clear; they show on the next Next/click | Show the words immediately (undoes the clear) |
| D19 | Unnumbered verses and digit keys | `n` = the nth Verse section as written, when no verse has a number | Digits do nothing; only `V` works |
| D20 | Interlude key and short name | `L` / "Int" (bare `N` stays free; FEAT-3's stage note moves to Alt+N) | `U`, or no Interlude key |
| D21 | Different look, same words (lines per slide, credits, background) | "The live song looks different now" banner with Update screen | Adopt such changes quietly at the next slide |

## Review notes

- The P3 scope finding (ship drag-reorder first, override later) is taken as a build order (§2 steps a–d) rather than removing the override from the spec, so the badge and copy rule are designed now and D10 can still drop it.
- The finding "resolve the N-key conflict in both specs" is resolved here by moving Interlude to `L` and stating the shared key rule; FEAT-3's own file is changed by its owner (§10 says what to change), since this pass edits only FEAT-4.
