# RustyPresenter — Bible Library Design

Date: November 3, 2026 · Status: written; technical choices are Claude's, product choices below were made by Jake
Builds on §8 (Bible) of `2026-10-29-rustypresenter-v1-design.md` and the native design in `2026-10-29-native-slint-design.md`. Code references are to commit c7a1e0b (`native-complete`).

## For Jake (plain words)

What you asked for: as many Bibles and languages as possible, the Septuagint, the Greek New Testament and the Hebrew Old Testament, two versions or two languages on screen at once, and never the New World Translation, the Joseph Smith Translation or The Passion Translation.

What it will be like:

- **KJV, WEB and BSB stay built in** and always work with no internet.
- **Settings → Bibles** gets a search box. Type "Spanish", "Reina", "Greek" or "Korean" and you see every Bible we are allowed to use, grouped by language. Click **Add**. It downloads once (a few megabytes, a few seconds) and then works offline forever, like KJV does today. You can remove one just as easily.
- **About 1,300 open Bibles in over 1,000 languages** come from eBible.org, a free, long-running Bible library. These are the ones we may legally keep on the computer.
- **The Hebrew Old Testament (Masoretic text), the Greek New Testament (several editions) and the Septuagint** come from the same place and are free to use. Hebrew shows right to left, as it should.
- **YouVersion and API.Bible**: if the church has its free key (it already can enter one today), the same search also lists every Bible that key may use, often hundreds more. Those stay online-only, because their publishers do not let apps keep whole Bibles. Each one is marked "Needs internet".
- **About "as many as we want" with YouVersion**: YouVersion does not give every Bible to every app. Each church makes its own free YouVersion app key, and for many Bibles the church must also accept that publisher's licence on platform.youversion.com. Without a key the YouVersion Bibles don't appear. **Question for you:** should the project apply for one shared RustyPresenter key so churches skip that step? Since the app is open source, anyone could copy a key built into it, so YouVersion might not allow that. Until you decide, each church uses its own key, as it does today.
- **Two Bibles at once**: when you add a passage you can tick "Also show" and pick a second Bible, for example English and Spanish, or the KJV next to the Greek. One switch chooses side by side or one above the other. The verses always line up; if one Bible has no such verse, that half stays blank instead of sliding out of line. You can set a default in Settings and change it on any passage later.
- **The three blocked translations never appear** anywhere: not in search, not in downloads, and if someone tries to import a file of one, the app calmly says it cannot be used.
- **Credits**: each Bible's copyright line appears where the app already shows credits, and both lines show when two Bibles are on screen.

We build this in four steps. After each step the app works fully and you can use what is done:

1. The library, eBible downloads, the blocklist and the new Bible picker.
2. Every Bible from YouVersion and API.Bible in the same search.
3. Original languages: the extra Old Testament books (Tobit, Maccabees and so on), Hebrew right to left, Greek, and typing book names in other languages.
4. Two Bibles at once.

## 1. Goals and non-goals

Goals
- Any open-licensed Bible in any language is one search and one click away, then offline.
- Every Bible a church's YouVersion or API.Bible key allows shows in the same search (online).
- Hebrew (RTL, pointed), polytonic Greek and the deuterocanon/Septuagint books display and can be typed.
- Parallel passages: two versions or languages, verse-aligned, side by side or stacked.
- The blocklist holds on every path.
- `data.json`, service files and existing `bibles/{ABBR}.json` keep working unchanged.

Non-goals (later list)
- Door43/unfoldingWord and CrossWire SWORD catalogs (§3.5).
- More than two versions at once.
- Morphology, Strong's numbers, footnotes, cross-references, audio Bibles.
- Bundling any new Bible text in the installer.

## 2. What the code does today (and what blocks us)

- Fixed lists: `TRANSLATIONS` (`src/bible.rs:44`), `BUILT_IN` and the `built_in()` match (`bible.rs:47-68`), YouVersion `VERSIONS` (`src/bible/youversion.rs:19`), `SOURCES` with English copyright constants (`src/bible/sources.rs:29-45`). `lookup` routes only translations in `SOURCES`.
- A 66-book Protestant canon with English names: `BOOKS`, `USFM` (`bible.rs:14-42`), OSIS table (`src/bible/import.rs:25-31`). Every importer drops books past 66 (`import.rs:379, 404, 468-470, 518, 533`). `Kjv::parse` throws away the apocrypha already present in the bundled `kjv.txt` and `web.txt` (`bible.rs:82`).
- Ids are 2-10 uppercase ASCII letters (`import.rs:58-63`), so Greek/Hebrew/Cyrillic abbreviations vanish and "RV" in two languages collides; same-id imports are merged book by book.
- Every imported Bible is parsed into memory at once (`import.rs:146-197`) and listed on every operator snapshot (`app.rs:416-453`, `src/ui/operator/view.rs:98-119`). Fine for 5 Bibles, not for 200.
- No language, script or direction anywhere; scripture is forced `Align::Left` (`src/render/layout.rs:147-148`); only Noto Sans Latin/Greek/Cyrillic is bundled.
- Scripture slides never carry a copyright footer; the reference line suppresses the footer (`layout.rs:121-128`).
- Picker: one chip per translation in a row (`ui/operator/topbar.slint:218-258`), Alt+1..6; the Add dialog has no picker; item edit has no translation control.
- Parsing (`parse_reference`, `find_book`, `display_name`) is English only; `find_book` measures prefixes in bytes.
- YouVersion/API.Bible caches are capped at 500 verses / half a book / 30 days (`sources.rs:94-109`). That stays: they cannot provide offline Bibles.

## 3. Sources

### 3.1 eBible.org catalog (primary, offline)

Catalog: `https://ebible.org/Scriptures/translations.csv` (UTF-8 with BOM, about 730 KB, 1,552 rows, 1,242 language codes, served via Cloudflare with ETag/Last-Modified). Columns used: `languageCode, translationId, languageName, languageNameInEnglish, title, description, Redistributable, Copyright, UpdateDate, OT/NT/DCbooks, textDirection, downloadable, shortTitle, script`.

Which rows the app offers:
- `Redistributable == True` **and** `downloadable == True`. Do not rely on 404s: `enggw_vpl.zip` answers 200 although GOD'S WORD is not redistributable.
- The text has a known licence (below). Unknown licence → not offered.
- Not blocked (§4).
- That leaves about 1,290 rows (214 full Bibles, about 1,077 full New Testaments, 32 with deuterocanon, 27 right-to-left).

Licence. The CSV has no licence column; `Copyright` is only "public domain" or a "Copyright © year holder" line. The licence is in `copr.htm` (inside the USFM zip) and `{id}_about.htm` (inside the VPL zip), as an `<a href>` to a `creativecommons.org/licenses/...` or `creativecommons.org/publicdomain/...` URL. The app reads the licence from those links at download time:

| Licence found | Offered | Notes |
|---|---|---|
| Public domain (the words "public domain", or a `/publicdomain/zero/` (CC0) or `/publicdomain/mark/` link) | yes | |
| CC BY, BY-SA, BY-ND, BY-NC, BY-NC-SA, BY-NC-ND (any version) | yes | Free app, text shown unchanged. Show the copyright line, licence name and link. Never charge. |
| NET (`engnet`) | yes, special | Credit ends with "(NET)" and links netbible.org (netbible.com/copyright). |
| None found / other | no | Download is cancelled and deleted; the row is hidden thereafter (§3.1.3). |

More than one licence link: all `creativecommons.org` links on the page are read. Links of the same kind (a deed and its legal code, two versions) count once, and the highest version is credited. If the kinds differ, the app credits the most restrictive one: the union of their NC, ND and SA terms, with the lowest version. A PD link next to a CC link counts as the CC link. Each case is pinned with a `copr.htm` / `_about.htm` fixture (§14).

Because the licence is only known after download, the catalog keeps a small **licence cache** (`bibles/catalog/licences.json`: id → licence + UpdateDate). A row with no cached licence is offered with "licence checked on download"; if the check fails the app says "This Bible's licence doesn't allow us to keep it. It has been removed." and hides the row. To avoid most surprises, the app ships a seed `assets/bibles/ebible-licences.json` generated by a maintainer script (`tools/ebible-licences`) from the current catalog (id → licence), refreshed each release. The seed is data about licences, not Bible text; installer cost about 60 KB.

Display abbreviation. The CSV has no abbreviation column (`shortTitle` is a title), so an eBible Bible's `abbr` is derived: the `translationId` with a leading `languageCode` removed (case-insensitive), then leading `-`/`_` trimmed, uppercased, at most 10 characters (`spaRV1909` → `RV1909`, `hboWLC` → `WLC`, `grcbrent` → `BRENT`, `eng-Brenton` → `BRENTON`, `tptNT` → `NT`). If nothing is left, the whole `translationId` is used, uppercased. The licence seed (below) may carry a curated `abbr` per id, which wins. Two installed Bibles may share an `abbr`; see §7.2.

Download format: `https://ebible.org/Scriptures/{id}_vpl.zip`, using the inner `{id}_vpl.xml` (`<v b="GEN" c="1" v="1">text</v>`, USFM book codes including deuterocanon, notes and headings already stripped). `{id}_about.htm` from the same zip gives the copyright and licence. Sizes: 1-6 MB per zip (engwebp 4.3 MB, grclxx 5.2 MB, hboWLC 4.6 MB). Verse ranges such as `v="15-16"` are stored on the first verse with the second left empty (`15` holds text, `16` = range continuation), so alignment still works. `_readaloud` (no verse numbers) is never used. The `_vpl.txt` flavour (BibleWorks codes) is only used by the bundled files.

Politeness: one download at a time, only on user action; User-Agent `RustyPresenter/{version} (+https://github.com/SuperDuperSoft/RustyPresenter)`; catalog refreshed with a conditional GET at most once a day and only when the library screen opens; a Bible is re-downloaded only when `UpdateDate` changes and the user clicks Update.

### 3.2 Original-language and historic texts

All from eBible.org through §3.1 (same download code), each checked against its `copr.htm` / details page:

| Text | eBible id | Licence | Notes |
|---|---|---|---|
| Hebrew OT, Westminster Leningrad Codex (Masoretic) | `hboWLC` | Public domain | RTL, points + cantillation. Hebrew versification. **Recommended Hebrew.** |
| Hebrew Masoretic OT (older WLC, 2013) | `hbo`, `hebwlc` | Public domain | Shown, not featured. |
| Septuagint, Brenton Greek with Apocrypha | `grcbrent` | Public domain | LXX numbering, 2 Esdras as EZR (23 ch, no NEH), ESG/DAG, SUS, BEL, 1ES, MAN, 3MA, 4MA. **Recommended Septuagint.** |
| "Septuaginta" | `grclxx` | Labelled public domain | No source edition named; carries an Orthodox Media Network notice. Offered (licence as labelled) with its own notice as credit; not featured. |
| English Septuagint, Brenton 1851 | `eng-Brenton` | Public domain | |
| Greek NT, SBLGNT | `grcsbl` | CC BY 4.0 | Credit "© 2010 Society of Biblical Literature and Logos Bible Software". **Recommended Greek NT.** |
| Greek NT, Textus Receptus | `grctr` | Public domain | |
| Greek NT, Byzantine / Patriarchal 1904 | `grcbyz` | Public domain | |
| Greek NT, Majority Text | `grcmt` | Public domain | |
| Greek NT, others | `grcsr`, `grctcgnt`, `grc-tisch` | Public domain | |
| Latin Vulgate (Clementine) | `latVUC` | Public domain | Vulgate psalm numbering; VPL has footnotes already stripped. |

The library's "Original languages" filter (step 3) features the recommended four (hboWLC, grcbrent, grcsbl, grctr) at the top.

Excluded on purpose: Rahlfs 1935 / CATSS LXX (CCAT user declaration, CC BY-NC-SA derivatives), Tyndale House GNT (all rights reserved), ETCBC Peshitta (CC BY-NC data, Text-Fabric), `grcf35` (copyrighted). Possible later upstreams: OSHB/morphhb (text PD, morphology CC BY 4.0), Robinson-Pierpont 2018 (Unlicense).

Hebrew display: cantillation (U+0591-U+05AF) is hidden by default and can be shown per Bible ("Show cantillation marks", off). Vowel points, maqaf (U+05BE) and sof pasuq (U+05C3) always stay. Hiding is done at display time, the stored text is never altered (no derivative).

### 3.3 YouVersion (online, church's own key)

The live catalog is added next to the fixed `VERSIONS` map, which stays for today's NIV/HCSB/CSB/NLT/BSB routes:
- `GET https://api.youversion.com/v1/bibles?language_ranges[]=*` with `X-YVP-App-Key`, paged (`page_size=99`, `next_page_token` → `page_token`, at most 60 pages). This is what the key may read. Response: `{data: [...], next_page_token, total_size}` (developers.youversion.com/api/bibles).
- `GET /v1/languages?bibles_available=true`, paged the same way, for `localized_name` and `text_direction` (`ltr`/`rtl`). `display_names` is left out with `fields[]` (it is large).
- `all_available=true` is **not** fetched: it doubles the calls against the app's roughly 200 requests an hour. Instead the page says "Don't see a Bible? Accept its licence at platform.youversion.com, then press Refresh list."
- Entry fields kept: `id, abbreviation, localized_abbreviation, title, localized_title, language_tag, copyright, promotional_content, publisher_url`. Credit = `copyright`, else `promotional_content` (HTML tags removed); if both are empty the Bible can't be added ("This Bible has no copyright line, so it can't be shown"), as YouVersion's own guidance asks.
- Cached in `bibles/catalog/youversion.json`, refreshed when older than 7 days (only when Settings → Bible is searched) or on **Refresh list**. A changed or removed key deletes the cached list. 429 → "YouVersion is busy. Try again in a minute."
- Text cache stays 500 verses / half a book / 30 days, unchanged from today. The YouVersion Platform terms are shown only after sign-in, so this rule could not be checked against them; it follows the strictest publisher rule (Biblica) and the 30-day API.Bible rule, and is to be checked when a maintainer signs in.
- Attribution: every YouVersion Bible's credit line is listed on the credits page, plus "Bible text from the YouVersion Platform (YouVersion.com)". The slide label shows the Bible's abbreviation, as for NIV today.
- The row tag is **"Needs internet · YouVersion"** (the critic's plain-words change; "keeps up to 500 verses offline" meant nothing to a volunteer). A passage added to a service is saved with it, as today, so a dropped connection never blanks a planned slide.
- Right-to-left Bibles (`text_direction` = `rtl`) stay hidden until step 3, like the eBible rows.

### 3.4 API.Bible (online, church's own key)

- `GET https://rest.api.bible/v1/bibles?include-full-details=true` once; everything returned is usable on the key's plan.
- Keep `id` (opaque, the real key), `abbreviation, abbreviationLocal, name, nameLocal, language{id,name,nameLocal,scriptDirection}, copyright`.
- Resolve by `id`, never by abbreviation (CSB is also a Spanish Bible). `pick_bible`'s `eng` preference stays only for old abbreviation-only items.
- Copyright persisted with the catalog (today `COPYRIGHTS` is in memory only, `api_bible.rs:88`).
- Starter plan: visible citation and link to https://api.bible on the credits page ("Scripture text from API.Bible, https://api.bible"); 30-day refresh rule unchanged; FUMS as today.
- Cached in `bibles/catalog/apibible.json`, refreshed when older than 7 days or on Refresh list; deleted when the key changes.
- Tag "Needs internet · API.Bible". `copyright` has its HTML tags removed. RTL (`scriptDirection` = `RTL`) hidden until step 3.

### 3.3.1 Added online Bibles

An online Bible the operator adds is kept in `bibles/online.json` with its own metadata (id, abbreviation, titles, language, direction, credit line, publisher link), so its credit and name survive a lost or refreshed catalog and a Bible YouVersion later drops. Its translation id is `yv:{id}` or `ab:{id}` (§5.1). Removing one used as the default falls back to KJV; one used in saved services asks first ("Used in N services. Their passages are saved and still show.").

A catalog row that is already one of today's routes (NIV 111, HCSB 72, CSB 1713, NLT 116, BSB 3034 on YouVersion; an English NIV/CSB/NLT on API.Bible) or a built-in Bible (English KJV, WEB, BSB) says "Already in your list" instead of Add, so the chips never show the same Bible twice.

Search runs in the core, off the UI thread: catalog files are parsed once into memory with a pre-normalised search string per row (§4 normalisation), typing is debounced 150 ms, and at most 80 rows (with language headers) come back, with "Type more to narrow the list" when more match. The page never builds thousands of rows.

### 3.5 Later

Door43/unfoldingWord (CC BY-SA, varied quality) and CrossWire SWORD (accept only PD/GPL/CC `DistributionLicense`; "permission granted to CrossWire" is not redistributable). Digital Bible Library needs an agreement. Not in these four steps.

## 4. Blocklist

`src/bible/blocklist.rs`, one function used everywhere:

```rust
pub fn blocked(abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool
pub fn blocked_row(lang: &str, abbr: &str, titles: &[&str], notices: &[&str], ids: &[&str]) -> bool
pub fn blocked_verse(book: u8, chapter: u16, verse: u16, text: &str) -> bool
```

`notices` are the copyright, publisher, description and promotional fields of every source (eBible about.htm, YouVersion `copyright`/`promotional_content`, API.Bible `copyright`, import headers): the strongest signal, because the NWT exists in 200+ languages and a title list will always leak. Notice patterns: NWT "Watch Tower", "Watchtower", "Wachtturm", "Torre de Vigía", "Tour de Garde", "jw.org", "Jehovah's Witnesses", "Testigos de Jehová", "Témoins de Jéhovah", "Zeugen Jehovas", "Testemunhas de Jeová"; JST "Herald Publishing House", "Community of Christ", "Reorganized Church of Jesus Christ of Latter Day Saints"; TPT "Passion & Fire Ministries" / "Passion and Fire Ministries", and "BroadStreet" together with "Brian Simmons". The bare word "passion" never blocks. `blocked_row` is `blocked` except that an abbreviation-only hit is ignored when the abbreviation equals the row's language code (eBible `tptNT`, any catalog's Tepehua "TPT").

**Step 2 revision (critic 13-20).** Normalisation adds, after the fold table, the digraph fold `ae`→`a`, `oe`→`o`, `ue`→`u`, so "Neue-Welt-Übersetzung" and "Neue-Welt-Uebersetzung" are one string; fold table also `ħ`→`h`. Every pattern goes through the same function, so patterns with spaces match. Abbreviations blocked (whole value, normalised): `nwt` + digits, `nwtr`, `nwtsty`, `rbi8`, `bi12`, `tnm`, `tmn`, `nwu`/`nwü`, `jst`, `iv`, `hsiv`, `tpt`. Titles: "inspired version" blocks alone (the old "with Holy Scriptures" rule was dead), plus "Joseph Smith's New Translation", "Restoration Edition", "Restoration Scriptures"; TPT book titles "Letters from Heaven", "Poetry on Fire", "La Pasión Traducción", "Traducción de La Pasión"; NWT in Afrikaans, Ukrainian, Greek, Romanian, Czech, Slovak, Hungarian, Swedish, Finnish, Turkish, Indonesian, Tagalog, Swahili, Vietnamese, Arabic and Hindi as well as the languages below. Known ids: `yv:1849` (TPT on YouVersion, from public listings; to verify against the live list with a key). Content fingerprints for online text: each fetched verse is checked (John 1:1 "the Word was a god", "Living Expression", "gospel preached through the Son"; "Jehovah" in Matthew 4:10 or Luke 4:8; Psalm 23:1 "my best friend"; Matthew 7:1 "judge not unrighteously"), and a hit refuses the passage with `bible.blocked`.

Normalisation: Unicode lowercase, NFKD with combining marks removed, then an explicit fold table for letters that have no decomposition (`ł`→`l`, `ø`→`o`, `æ`→`ae`, `œ`→`oe`, `đ`/`ð`→`d`, `ß`→`ss`, `ı`→`i`, `þ`→`th`), then non-alphanumerics removed. The patterns below go through the same `normalise()` in code (built once in a `LazyLock`), so no pattern is ever folded by hand. Each listed language has a unit test using the real native title ("Przekład Nowego Świata", "Ny Verden-oversættelsen", "신세계역 성경", "新世界譯本", ...).

Matches (any one blocks):
- **Abbreviation, whole value only**: `nwt`, `jst`, `tpt`. `iv` and the localized NWT abbreviations `tnm` (Spanish, Portuguese, Italian), `tmn` (French) and `nwu` (German NWÜ) block *only together with a title hit*, or in online catalogs (see below).
- **Title contains**: "New World Translation" (also written "NewWorld Translation"; normalisation makes both one string) and its translated titles: Spanish "Traducción del Nuevo Mundo", Portuguese "Tradução do Novo Mundo", French "Traduction du monde nouveau", German "Neue-Welt-Übersetzung", Italian "Traduzione del Nuovo Mondo", Dutch "Nieuwe-Wereldvertaling", Polish "Przekład Nowego Świata", Danish "Ny Verden-oversættelsen", Norwegian "Ny verden-oversettelsen", Swedish "Nya världen-översättningen", Finnish "Uuden maailman käännös", Russian "Перевод нового мира", Korean "신세계역", Japanese "新世界訳", Simplified Chinese "新世界译本", Traditional Chinese "新世界譯本"; "Joseph Smith Translation", "Inspired Version" (with "Holy Scriptures" or "Joseph Smith"), "The Passion Translation", "Passion Translation". The list lives in one table in `blocklist.rs`, so a language is one line and one test.
- **Content fingerprint**, for files and pasted text with no title (USX and OpenSong name nothing, and the operator types the abbreviation): `blocked_text(verses)` checks a few distinctive verses per blocked translation after normalisation. NWT: John 1:1 ending "the Word was a god" (or the translated form of that clause in a listed language), and "Jehovah" (or a listed local form) in 20 or more New Testament verses. TPT: Psalm 23:1 containing "my best friend". JST: Genesis 50 longer than 30 verses, or Matthew 7:1 reading "judge not unrighteously". Two NWT signals, or one TPT or JST signal, block. Tests pin that KJV, WEB, BSB and fixtures of other translations are not blocked.
- **Known ids**: a small table filled in from live listings (YouVersion/API.Bible ids for NWT/TPT found when step 2 is built; none exist in eBible today).

False-positive guard: **never match on language code or id prefix**. eBible `tptNT` is Tlachichilco Tepehua (ISO 639-3 `tpt`) and must stay. An abbreviation-only hit on `TPT`/`JST`/`NWT` blocks only when the entry is not an eBible row whose `languageCode` equals that abbreviation; for online catalogs an abbreviation-only hit blocks (safe side). Unit tests pin `tptNT` as allowed.

Applied in: catalog build and search (all sources), download (metadata and content), `import::parse`/save (`import.rs:203`: metadata including the operator's abbreviation, and content), paste (`app.rs:636-640`: abbreviation and content), `settings/translation` (`app.rs:741-745`), `service/add` and `service/update` (`app.rs:547-571`: these take whole scripture items over the HTTP action API and never pass lookup, paste or import, so an item whose translation or verses are blocked is refused), loading a saved service (such an item is dropped and the operator sees "One passage used a translation that can't be used in RustyPresenter and was removed."), YouVersion bare-number `version_id`, API.Bible `bible_id`/`pick_bible`, the "Also show" picker, and when loading an existing `bibles/*.json` (a blocked file already on disk is skipped and listed under "Can't be used").

Behaviour: catalogs hide blocked entries silently. A typed id, paste or import is refused with the string `bible.blocked`: "This translation can't be used in RustyPresenter. Please choose another Bible."

## 5. Data model

### 5.1 Translation ids

A **translation id** is a stable string, separate from the display abbreviation:

| Kind | Id | Example |
|---|---|---|
| Built-in | the abbreviation | `KJV`, `WEB`, `BSB` |
| Existing import | the abbreviation | `HCSB` |
| eBible download | `ebible:{translationId}` | `ebible:spaRV1909`, `ebible:hboWLC` |
| YouVersion | `yv:{id}` | `yv:111` |
| API.Bible | `ab:{id}` | `ab:de4e12af7f28f599-02` |
| Today's provider routes | unchanged | `NIV`, `ESV`, `NLT`, `CSB` |

Old values (`"translation":"KJV"` in services and Settings) mean what they mean today, so no migration is needed. Ids are **opaque and case-sensitive**: only a legacy bare abbreviation (no `:`) is ever uppercased. Every place that uppercases today (`bible/paste` at `app.rs:640`, `api_bible.rs:91, 142, 147, 162`, `sanitize_abbr`) goes through one `bible::normalise_id()` that leaves a value with a `:` prefix unchanged. A test round-trips `ebible:spaRV1909` through `bible/paste`, Settings and a saved service. File names encode the id: `:` → `~`, other characters outside `[A-Za-z0-9._-]` → `%XX`; existing `bibles/{ABBR}.json` keep their names.

### 5.2 Metadata

`bibles/index.json` (new) is only a cache. The truth is each Bible's own file: downloads store their `BibleMeta` in a `meta` field next to the verses (§6), so licence, source, `update_date`, `lang`, `dir` and `script` survive a lost index. At startup the index is checked against the directory listing: files missing from the index are read and added, entries whose file is gone are dropped, and a damaged index is rebuilt from the files. This also repairs a crash between writing a file and updating the index, or between deleting a file and its entry.

```rust
pub struct BibleMeta {
    pub id: String,             // translation id (§5.1)
    pub abbr: String,           // shown on chips, any script ("RV1909", "ΚΔ")
    pub name: String,           // local title
    pub name_en: String,        // English title, may equal name
    pub lang: String,           // BCP 47 ("es", "hbo", "grc")
    pub lang_name: String,      // in the language itself ("Español")
    pub lang_name_en: String,   // "Spanish"
    pub dir: Dir,               // Ltr | Rtl
    pub script: String,         // ISO 15924 if known ("Hebr", "Grek", "Latn")
    pub versification: Vsys,    // §5.4
    pub books: Vec<u8>,         // book indices present
    pub book_names: Vec<(u8, String, String)>, // index, long, short (local)
    pub copyright: String,      // credit line exactly as given
    pub licence: Licence,       // Pd | Cc { kind, version, url } | Net | Provider
    pub source: Source,         // Builtin | Import | Ebible { update_date } | YouVersion | ApiBible
    pub bytes: u64,
}
```

All fields `#[serde(default)]`; an old import gets `lang = "und"`, `dir = Ltr`, `versification = English`, `source = Import`. The language-code guard of §4 reads `lang` from the file's own `meta`, so a Tepehua `TPT` row is still allowed after an index rebuild.

### 5.3 Books past 66

Append to `BOOKS`/`USFM`/OSIS after index 65 so stored `u8` indices never change: TOB JDT ESG WIS SIR BAR LJE S3Y SUS BEL 1MA 2MA 3MA 4MA 1ES 2ES MAN PS2 ODA PSS EZA 5EZ 6EZ DAG LAO (indices 66-90). eBible `.txt` aliases (PRA, PRM, 4ES, DNG, PSX…) map onto these. New group "Deuterocanon / Apocrypha" in the book grid, shown only when the selected Bible has such books. Every `USFM[book]` / `BOOKS[book]` index becomes a checked `get` (YouVersion, API.Bible, `human_query`). The half-book cache limit uses the selected Bible's own chapter count, not KJV's. `Kjv::parse` and all importers keep the extra books. The 66-book test asserts become "first 66 unchanged".

### 5.4 Versification

Each Bible stores its scheme (`Vsys`): `English` (KJV-style), `Original` (Hebrew/Masoretic), `Septuagint`, `Vulgate`. Detected from the text on import/download (e.g. MAL has 3 chapters → Original/Septuagint; PSA 22:1 = "Κύριος ποιμαίνει με" → Septuagint) with eBible id overrides for the known texts.

`src/bible/versify.rs` maps any (book, chapter, verse) to the **English** reference and back, using rule tables written for the app from the published Paratext/ubsicap `.vrs` mappings (org.vrs, lxx.vrs, vul.vrs, eng.vrs). Their licence must be checked before any file is copied; if it does not allow reuse, the tables are written by hand from the documented differences, which are facts, not text. Covered at minimum: LXX/Vulgate psalm numbering (9-147), psalm titles as verse 1, Mal 3:19-24 ↔ 4:1-6, Joel 2:28-3:21 ↔ 3-4, LXX 2 Esdras ↔ Ezra + Nehemiah, Daniel/Esther additions, Jeremiah LXX order.

Use: what the operator types is always the **selected Bible's own numbering** (a Greek reader typing Ps 22 in the LXX gets "The Lord is my shepherd"). For parallel passages the second Bible is looked up by mapping through English (§11). A verse that does not exist comes back empty, never shifted.

## 6. Storage

```
{data}/bibles/
  index.json                      metadata for every installed Bible (§5.2)
  HCSB.json                       existing imports, unchanged format
  ebible~spaRV1909.json           downloads: same tuple format as imports
  catalog/
    ebible.csv  ebible.etag       last catalog + ETag
    licences.json                 licence cache (§3.1)
    youversion.json  apibible.json
  cache/                          provider text caches (unchanged rules)
```

- Same `[(book, chapter, verse, text)]` JSON format as imports for backward compatibility, plus an optional `meta` object (`BibleMeta`, serde default) that old files lack and old readers ignore; files are typically 4-6 MB. (Compression is not worth the format change now.)
- A `translationId` from the CSV is used only if it matches `^[A-Za-z0-9_-]{1,40}$`; other rows are skipped. The partial file name uses the same id encoding as the final file (§5.1), and Windows device names (CON, PRN, AUX, NUL, COMn, LPTn) are refused as in `sanitize_abbr`.
- Downloads go to `bibles/.partial/{encoded id}.zip`, are checked (zip opens, `{id}_vpl.xml` parses, has ≥ 1 book, licence OK, not blocked by metadata or content), then written with `store::write_atomic`, then added to `index.json`. A failed or cancelled download leaves nothing behind, and `bibles/.partial/` is emptied at startup, so a crash or forced quit mid-download leaves nothing either.
- Hard caps: the download is read through `take(50 MB)` and aborted past it (at once when `Content-Length` is over the cap); `{id}_vpl.xml` is inflated through `take(200 MB)` and the job fails if the entry is larger. Free space is checked before the download (zip size × 5, or 250 MB with no `Content-Length`) and again every 5 MB as bytes arrive; "Not enough disk space for this Bible (needs about 25 MB)".
- **Update** downloads to `.partial/` as above and replaces the installed file and index entry only when the new copy passes every check. If a check fails, the installed copy stays and keeps working, and the message says so (§13).
- **Lazy loading**: replace `load_all`/`loaded` with `index.json` for lists and an LRU of at most 4 parsed Bibles (built-ins are separate statics). Snapshots carry only installed Bible metadata (abbr, name, lang, dir), never catalog rows.
- Remove deletes the file and the index entry. Services that used a removed Bible keep their snapshotted verses; a new lookup says "That Bible was removed. Add it again in Settings → Bibles."
- `clear_cache` walks `bibles/cache/` by provider, so caches of catalog Bibles are cleared when a key changes (API.Bible terms §10).

## 7. Bible library UI

### 7.1 Settings → Bibles

One page replaces today's flat list (`ui/dialogs/settings-bible.slint`):

- **Search box** at the top, focused on open: matches title, local title, abbreviation, language (local and English) in any script, accent-insensitive.
- **Language filter** drop-down ("All languages", then the church's own language, then languages A-Z by English name with the local name after it) and a chip row: **Installed · Original languages · All**.
- Results grouped by language. Each row: abbreviation, title, a short tag ("Offline", "Online · YouVersion", "Full Bible", "New Testament", "Includes Apocrypha"), size, and **one button**: Add / Remove / Update / Cancel. Hebrew and other RTL titles display right to left.
- **Progress** in the row: a bar with "Downloading… 2.1 of 4.3 MB", then "Getting ready…", then "Added". Only one download runs; a second Add is queued and shows "Waiting".
- **Default Bible** and (step 4) **"Also show" default** with **Side by side / Top and bottom** at the top of the page.
- **Credits** section: every installed Bible's full copyright and licence with link, plus the API.Bible citation.
- Online catalogs appear only when the key is set; otherwise one line: "Add a YouVersion or API.Bible key to see hundreds more Bibles (online)." linking to the existing key settings.
- Offline: installed Bibles still list; search shows the last saved catalog with "You're offline. Bibles you've added still work." and Add buttons disabled.

Picking a Bible never asks anything else. Add is one click.

### 7.2 Picker in the Bible box and Add dialog

The chip row (`topbar.slint:218-258`) becomes:
- the **current Bible chip** (opens the picker), then up to 4 chips for **favourites, then recent** (last 4 used). Alt+1..5 maps to these visible chips.
- **Picker pop-up**: search box, sections **Favourites · Recent · Installed (by language)**, star to favourite, "Find more Bibles…" at the bottom opens Settings → Bibles with the search text carried over.
- The Add dialog shows the same picker next to the reference (today it uses the global translation only, `add.rs:88-89`).
- Item edit for scripture gets the same picker; changing it re-looks up the verses (`bible/lookup`).
- Settings stores `favourites: Vec<String>` and `recent: Vec<String>` (serde default).
- Equal abbreviations (an import `RV1909` next to the download of the same name, or several Reina-Valera rows) are told apart wherever they appear together (chips, picker, Settings list) by adding the language code, then the source: "RV1909 · es", "RV1909 · es · imported". Slides keep the plain `abbr`. Lookups always use the id, never the abbreviation.

## 8. Typing references in other languages

- Normalise first: full-width and Arabic-Indic/Devanagari/other Unicode decimal digits → ASCII; `：` `﹕` → `:`; `，` → `,`. A comma between chapter and verse ("Иоанна 3,16") is read as `:` when the text has no `:`.
- Book lookup order: the selected Bible's own names (long, short, from the file) → every installed Bible's names → English names and aliases. Prefix matching counts characters, not bytes (`find_book`, `bible.rs:142-149`), minimum 3 characters for Latin/Greek/Cyrillic, 1 for CJK/Hebrew where a single character is a book abbreviation only if it is an exact short name.
- Book names come from the files: eBible VPL has none, so downloads also fetch names from the eBible `{id}_usfm.zip` `\toc1`/`\toc2`/`\h` lines only when the step needs them (step 3); until then English names are used. Importers stop discarding USX `toc1-3`, Zefania `bname/bsname`, OSIS/OpenSong `n=`.
- Labels: the slide reference uses the Bible's own book name ("Juan 3:16", "יוחנן 3:16"). The verse keeps structured `(book, chapter, verse)` fields so labels are never parsed back (`join_refs`, `verse_num` in `slides.rs` use them instead).

## 9. Rendering

### 9.1 Fonts (bundled, all SIL Open Font License 1.1)

| Font | Covers | Size | Why |
|---|---|---|---|
| Noto Sans (existing) | Latin, polytonic Greek, Cyrillic | — | Already bundled |
| Noto Serif Hebrew Regular + Bold | Hebrew with points and cantillation | about 0.1 MB | Matches Noto Sans; GPOS mark placement |
| Gentium (7.000) Regular + Bold | Polytonic Greek, Latin, Cyrillic | about 1.8 MB | Best-tested polytonic Greek; used for Greek Bibles |
| Ezra SIL 2.51 | BHS-style Hebrew | about 0.2 MB | Used when "Show cantillation marks" is on |

Each font ships with its OFL.txt in `assets/fonts/` and is listed in the About credits. Licences are checked against the files when bundling (Noto Hebrew's licence page could not be loaded during research; the font files carry OFL in their name table). Not used: SBL Hebrew/Greek (custom EULA, not OFL). Other scripts (Arabic, Devanagari, Ethiopic, CJK, Hangul, Thai…) use system fonts as today; Settings → Bibles shows "Your computer may not have a font for this language" when the shaper reports missing glyphs in a test line (the row's title, and its first verse once downloaded). This check ships in step 1 and is shown on the row before Add. Downloadable per-script Noto fonts are a later item.

Font choice per Bible: `meta.script` → Hebr: Noto Serif Hebrew (Ezra SIL with cantillation); Grek: Gentium; others: the theme font with system fallback. The theme font is still used for the reference line and credits.

### 9.2 Direction and layout

- `SlideContent::Text`, `SlideLayer::Text`, `TextSlide`, `RenderKey` gain `dir: Dir` and `lang: String` (serde default Ltr/"").
- RTL scripture is `Align::Right` (or `End`), LTR stays `Align::Left`. cosmic-text 0.19 with `Shaping::Advanced` does bidi; each paragraph gets the Bible's direction forced, not guessed.
- Verse numbers: superscript digits with a thin space; in RTL paragraphs each number is wrapped in RLM marks so it stays at the start of its verse. Raster tests cover Gen 1:1 Hebrew and Ps 23 both ways.
- Slide splitting stops assuming Latin: break at Unicode line-break opportunities (`unicode-linebreak`, already a cosmic-text dependency), sentence punctuation includes `。，；：！？ ، ؛ ؟ ׃ · ;`, and the 330-character auto limit is weighted (wide East Asian characters count 2).
- Reference and footer can show together: reference top-right as today, credit footer bottom. Same in `web/render.js` while it remains.
- Stage (`ui/stage.slint`) and the Slint preview get the direction and font via the slide model.

## 10. Credits

- Scripture slides show the Bible's short credit (abbreviation + copyright line, truncated to one line on the slide, full on the credits page) on the first or last slide following the existing `settings.credits` ("first"/"last"/"none") that today applies only to songs.
- CC texts: credit line + licence short name ("CC BY-SA 4.0"); full link on the credits page. NET: "(NET)" ending. YouVersion: `copyright` or `promotional_content`. API.Bible: copyright plus the api.bible link on the credits page.
- "none" still hides slide credits for public-domain texts only; texts that require attribution always show it on one slide of the passage, and Settings explains: "Some Bibles require their credit to be shown; it appears once per passage."
- Parallel passages show both credits, each under its own half.

## 11. Parallel Bibles

Model (all serde default, so old services load):

```rust
// ItemKind::Scripture gains:
pub also: Option<AlsoShow>,
pub struct AlsoShow { pub translation: String, pub verses: Vec<VerseText> }
pub layout: ParallelLayout,           // SideBySide | Stacked
// VerseText gains: book: u8, chapter: u16, verse: u16 (0 = unknown, old items)
// Settings gains: also_show: Option<String>, parallel_layout: ParallelLayout
```

- **Lookup**: `bible/lookup` takes an optional `also`; one request returns both lists. The second list is built by mapping each first-Bible verse through English versification (§5.4) to the second Bible's numbering; missing → empty text.
- **UI**: Bible box and Add dialog show "Also show: [none ▾]" under the picker (same picker), and a two-button switch **Side by side | Top and bottom**. Item edit has the same two controls. Defaults come from Settings.
- **Slides**: grouping in `scripture_slides` uses, per verse, the longer of the two texts after weighting, so both halves split at the same verses. Each half has its own font, direction and credit. Both halves use the same font size (the smaller of the two fits). Side by side: two columns with a gap, the RTL column placed on the right. Stacked: first Bible on top. `MIN_PX` 26 still applies; if a single verse cannot fit, it splits on its own slide.
- **New content shape**: `SlideContent::Scripture { a: Block, b: Option<Block>, layout }` where `Block { text, dir, lang, reference, credit }`, threaded through live, cache key, layout, preview, thumbnails.
- **Stage** shows both texts stacked (readable on a small monitor). **Phone remote** shows both.

## 12. Phone remote

- `server.rs` slide payload gains `dir`, `lang`, and for parallel slides `text2`, `dir2`, `lang2`.
- `web/remote.js` renders each with `dir` and `lang` attributes (`dir="auto"` when unknown) and both blocks stacked.
- The remote never sees catalogs or downloads.

## 13. Errors and offline (plain words, all in `src/strings/`)

| Case | Message |
|---|---|
| No internet when opening the library | "You're offline. Bibles you've added still work." |
| Download fails | "Couldn't download {name}. Check the internet and try again." (Retry button) |
| Licence check fails | "This Bible's licence doesn't allow us to keep it. It has been removed." |
| Update fails a check | "The update for {name} couldn't be used. The copy you have still works." |
| Download too large | "This Bible is larger than expected, so it wasn't added." |
| Blocked translation | "This translation can't be used in RustyPresenter. Please choose another Bible." |
| Not enough disk | "Not enough disk space for this Bible (needs about {n} MB)." |
| eBible catalog unreadable | "The Bible list couldn't be read. Showing the last saved list." |
| Online Bible, no internet, verse not cached | "{abbr} needs the internet for this passage. Try KJV, or a Bible you've added." |
| Verse missing in the second Bible | (no message, that half is blank) |
| Removed Bible | "That Bible was removed. Add it again in Settings → Bibles." |
| No font on this computer | "Your computer may not have a font for this language." |

Downloads run as jobs (existing job/toast system); closing Settings does not cancel them. Sunday safety: nothing in the library runs automatically during a service; no catalog fetch while a slide is live unless the user opened the library.

## 14. Tests

Unit tests use no network; anything online is behind `RP_NET_TESTS=1`.

- Blocklist: NWT/JST/TPT by abbreviation, English title, each translated title written natively (incl. ł, ø, æ, Hangul, Traditional Chinese), accents/case; localized abbreviations only with a title hit; content fingerprint on a titleless USX/OpenSong fixture saved under another abbreviation; KJV/WEB/BSB not fingerprinted; `tptNT` (Tepehua) allowed, also after an index rebuild; import, paste, settings, `service/add`, `service/update` and service load refuse.
- eBible CSV parse from a trimmed fixture (BOM, quoted commas, Redistributable/downloadable filters, RTL rows).
- Licence parse from `copr.htm` / `_about.htm` fixtures: PD, CC0, PD Mark, each CC kind incl. BY-NC-SA, several links of one kind, links of different kinds (most restrictive wins), NET, none.
- VPL XML parse: USFM codes incl. deuterocanon, verse range `15-16`, empty verses.
- Download pipeline with a local zip fixture: partial file cleaned on error and at startup, atomic write, index updated, blocked/unlicensed refused, over-cap download and over-cap inflated entry refused, hostile `translationId` (`../x`, `CON`) refused, failed Update keeps the installed copy.
- Ids: old `KJV`/`HCSB` values resolve; `ebible:spaRV1909` keeps its case through paste, Settings and services; file-name encoding round-trips; existing `bibles/*.json` load unchanged; derived `abbr` for the §3.1 examples; equal abbreviations told apart.
- Index: rebuilt from the files with licence and source intact; a file missing from the index is added; an entry with no file is dropped.
- Lazy loading: 50 fixture Bibles → snapshot does not parse them; LRU holds at most 4.
- Books past 66: indices 0-65 unchanged; Tobit parses; providers guard indices; bundled KJV/WEB apocrypha load.
- Versification: Ps 23 Eng ↔ Ps 22 LXX, Mal 4:1 ↔ Mal 3:19, Joel 2:28 ↔ 3:1, psalm title verse shift; missing verse → empty.
- References: "Juan 3:16", "Иоанна 3,16", "ヨハネ ３：１６", Hebrew name, character-based prefix.
- Rendering (bundled-only fonts): Hebrew Gen 1:1 right aligned with no tofu, polytonic Greek Jn 1:1 no tofu, verse numbers in RTL, CJK splits.
- Parallel: aligned grouping, empty cell, both credits, side/stacked raster, stage and remote payloads.
- Catalog JSON parse for YouVersion/API.Bible fixtures; credit fallback; refuse uncredited text.
- Net (gated): fetch translations.csv, download `grcsbl_vpl.zip` and check licence = CC BY 4.0; YouVersion/API.Bible listing with test keys.

## 15. Build steps

Each step ends with all tests passing and the app fully usable.

### Step 1 — Library, eBible downloads, blocklist, picker
1. `bible/blocklist.rs` with tests (incl. `tptNT`, fold table, content fingerprint); wire into import, paste, `settings/translation`, `service/add`/`service/update`, service load, YouVersion `version_id`, API.Bible `bible_id`/`pick_bible`, loading existing files.
2. Translation ids (§5.1) with case kept for prefixed ids, and id → file-name encoding; old values keep working.
3. `BibleMeta` stored in each downloaded file, `bibles/index.json` as a cache checked against the directory at startup; lazy LRU loading replacing `load_all`/`loaded`; snapshots carry metadata only.
4. `bible/catalog/ebible.rs`: CSV fetch (conditional GET, ETag), parse, filter, search, language grouping; licence seed `assets/bibles/ebible-licences.json` + `tools/ebible-licences` script; licence cache.
5. VPL zip download job: id check, progress, cancel, queue, size caps, disk checks, licence check from `_about.htm`, partial-file cleanup (also at startup), atomic write, index update; remove, and update that keeps the installed copy on failure.
6. Settings → Bibles page (§7.1): search, language filter, Installed/All, one-button rows, progress, missing-font warning, default Bible, credits list, offline state.
7. Picker (§7.2): current chip + favourites/recent chips, search pop-up, "Find more Bibles…"; Add dialog picker; item edit picker with re-lookup; Alt+1..5.
8. Credits on scripture slides per `settings.credits`, reference + footer together; credits page shows licences.
9. Strings in `src/strings/`, tests (§14 relevant items), release build and smoke run with a temp data dir.
(Right-to-left rows, meaning `textDirection` RTL or script Hebr, Arab, Syrc or Thaa, are hidden from the catalog in step 1, because scripture is still left aligned and no Hebrew font is bundled; they appear in step 3 with RTL layout and the Hebrew font. Greek downloads are offered in step 1 with the theme font, since Noto Sans covers polytonic Greek. The "Original languages" filter appears in step 3.)

### Step 2 — YouVersion and API.Bible full catalogs
Step 2 was built before step 1's code, so it carries the parts of step 1 it needs: `bible/blocklist.rs` (§4, with the step 2 revision) wired into lookup, paste, import, `settings/translation` and both catalogs; prefixed ids `yv:`/`ab:` kept case-sensitive with file-name encoding for caches; and the search box at the top of Settings → Bible. eBible rows join the same search when step 1 lands.
1. YouVersion catalog client: paged `/v1/bibles?language_ranges[]=*`, `/v1/languages` for names and direction (no `all_available`, §3.3); cache file; 7-day refresh; 429 handling.
2. API.Bible catalog client: `/v1/bibles?include-full-details=true`, persisted copyrights, id-based resolution.
3. Route `yv:{id}` and `ab:{id}` translations through `lookup` and the existing cache rules; cache paths and `clear_cache` cover catalog ids.
4. Merge both catalogs into the same search, grouped by language, tagged "Needs internet · YouVersion/API.Bible"; Add = add to `bibles/online.json` (no download); Remove asks first when services use it.
5. Credits: YouVersion copyright/promotional fallback and refusal; API.Bible citation link.
6. Blocklist applied to both catalogs; fill the known-ids table from live listings.
7. Fixture tests; `RP_NET_TESTS` checks; keep today's NIV/ESV/NLT/CSB/HCSB routes working.

### Step 3 — Original languages
1. Append books 66-90 to `BOOKS`/`USFM`/OSIS with aliases; guard every index; keep apocrypha in `Kjv::parse` and all importers; per-Bible chapter counts for cache limits; book grid "Deuterocanon / Apocrypha" group.
2. `bible/versify.rs` with English/Original/Septuagint/Vulgate tables; detection on import/download plus eBible overrides; tests.
3. Bundle Noto Serif Hebrew, Gentium, Ezra SIL with OFL files; per-script font choice.
4. `dir`/`lang` through `SlideContent` → live → `RenderKey` → layout; RTL alignment, RLM around verse numbers; stage and preview; phone remote `dir`/`lang`; Unicode line breaking and weighted length in slide splitting.
5. Hebrew cantillation toggle (display-time only).
6. Localized book names: capture `\toc1/\toc2/\h` (USFM zip for eBible downloads, USX/Zefania/OSIS/OpenSong for imports); store in `BibleMeta`; digit/punctuation normalisation; per-Bible name lookup in `parse_reference`; character-based prefixes; localized slide labels; structured verse keys replace label re-parsing.
7. "Original languages" filter with the recommended texts featured; show the right-to-left catalog rows hidden in step 1; raster tests for Hebrew and Greek with bundled-only fonts.

### Step 4 — Parallel Bibles
1. Model: `AlsoShow`, `ParallelLayout`, structured `VerseText` keys, Settings defaults (all serde default).
2. `bible/lookup` with `also`: map through versification, empty for missing verses.
3. `SlideContent::Scripture { a, b, layout }`; aligned grouping using the longer text; shared font size; side-by-side (RTL column right) and stacked layouts; both credits.
4. UI: "Also show" picker + Side by side / Top and bottom switch in the Bible box, Add dialog, item edit; defaults in Settings → Bibles.
5. Stage shows both stacked; phone remote shows both with direction.
6. Tests: alignment, missing verse, credits, raster for both layouts incl. English + Hebrew, stage and remote payloads; smoke run on room screen and stage.
