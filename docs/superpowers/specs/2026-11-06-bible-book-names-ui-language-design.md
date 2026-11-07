# Bible references in the UI language (book names) — design note

Date: November 6, 2026 · Status: note only, not built. Builds on Bible library spec §8
(`2026-11-03-bible-library-design.md`) and the I18N spec (`2026-11-06-i18n-any-language-design.md`).

## For Jake (plain words)

Today the Bible box only understands English book names ("John 3:16", "Jn 3:16"). A Spanish user typing
"Juan 3:16" gets "I don't recognize the book". This note plans how the Bible box will also understand book names in
the language the app is shown in, and in the language of the Bible being used. English always keeps working.

## Why it is bigger than song section names

- 66 books, each with a long name and several short forms, so translators supply about 66 entries, not 8.
- Spec §8 already wants a different, larger thing: the **Bible file's own** book names (USX `toc1-3`, Zefania
  `bname`, OSIS `n=`; eBible needs a second download), labels in that Bible's names ("Juan 3:16" on screen), and
  verses that keep `(book, chapter, verse)` so labels are never parsed back. The UI-language names should slot into
  the same lookup, not be a second, separate one.
- Labels and references are built in many places (`bible::display_name`, 12 callers; `canonical`, `join_refs`,
  `verse_num` parse labels back today), so a localised label touches the scripture planner, print, stage, phone and
  service files.

## Plan

1. **Names in the catalog.** `src/bible/names.rs`: `fn ui_book_names(book: u8) -> Vec<String>` built from
   `trc!("bible book", "Genesis")` … (66 literal calls, one per book, like the song heading words in
   `sections::kind::translated_heads`). Translators may give several forms split by `|` ("Juan|Jn"). `docs/TRANSLATING.md`
   explains the `bible book` context.
2. **Normalise input** as §8 says (full-width and other Unicode digits → ASCII, `：` → `:`, "Иоанна 3,16" comma).
3. **Lookup order** (`find_book_in(name, sources)`): the selected Bible's own names → every installed Bible's names →
   the UI language's names → English names and aliases. Exact match first, then a unique prefix counted in
   characters (minimum 3 for alphabetic scripts, exact only for a single CJK/Hebrew character). An ambiguous prefix
   across sources is an error that lists the choices.
4. **Display.** Phase 1 keeps the English label on slides (no change to stored data). Phase 2 (with §8's structured
   verse ids) shows the Bible's own name, else the UI language's name, else English; `join_refs`/`verse_num` read
   `VerseText.id` instead of parsing the label.
5. **Phone remote**: `/i18n.json` already carries Rust strings; the phone's Bible box sends text to the core, so it
   needs no change.

## Tests

`juan_3_16_in_spanish` (with `i18n::testing::with_catalog`), `english_always_works`, `ambiguous_prefix_lists_choices`,
`fullwidth_digits_and_colon`, `cyrillic_comma_reference`, `bible_book_names_reach_the_template` (extractor finds
all 66 `bible book` entries).

## Size

Phase 1 (steps 1–3, English labels): about a day, mostly `src/bible.rs` and a new `src/bible/names.rs`. Phase 2 (step 4)
belongs with spec §8's importer work and touches the scripture planner, print and the stage.
