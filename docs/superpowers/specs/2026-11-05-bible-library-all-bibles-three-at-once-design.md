# RustyPresenter — Every Bible We Can Find, Three at Once

Date: November 5, 2026 · Status: approved by Jake (November 6, 2026)
Changes `2026-11-03-bible-library-design.md` (the "Library spec", on branch `v1-candidate`) and `2026-11-01-feat5-bible-extras-design.md` (the "FEAT-5 spec", on branch `feat5-core`). Anything not mentioned here stays as those specs say. Where this file disagrees with them, this file wins.

## For Jake (plain words)

- **Three Bibles at once.** "Also show" takes up to 2 extra Bibles, in any language. Side by side makes three columns; top and bottom makes three rows. Verses line up. If a Bible lacks a verse, its space stays blank and the volunteer gets a note.
- **Every Bible we can find.** One search box covers eBible.org, YouVersion, API.Bible and two more open libraries (Door43 and CrossWire). You click Add once. Offline when the licence allows it, "Needs internet" when it doesn't.
- **Adding a future library is one small module.** The search, Add button and blocker never change.
- **No bad versions.** The New World Translation, Joseph Smith Translation and The Passion Translation stay blocked. Every Bible from every library has to pass the blocker. Adding another blocked version is a one-line change.
- **What "find" can't do.** We only use Bibles we have the right to use. YouVersion shows only what the church's own free key allows, and some Bibles won't be offered to apps at all.
- **One change from what you read on Nov 3.** Online Bibles (NIV and friends) aren't stored forever in services, because publishers cap how much an app may keep. The words go in a small capped store, and the app warns before Sunday if a passage isn't ready offline.

## 1. Goals and non-goals

Goals
- One to three Bibles on a slide, any mix of languages and directions.
- One parallel feature. FEAT-5 §3.2 and Library §11 become this spec.
- All lawful sources in one search, each behind one interface.
- The blocklist holds on every source and every path.

Non-goals
- Four or more Bibles at once.
- Scraping bible.com or any site whose terms forbid it.
- Sharing one YouVersion key across churches. A key in an open-source app can be copied by anyone, so each church keeps its own.
- Bible Brain and the Digital Bible Library for now (§4).

## 2. Three at once: data

Replaces Library §11's `Option<AlsoShow>` and FEAT-5's `second: Option<SecondText>`. Neither has shipped, so nothing needs converting.

```rust
pub const MAX_ALSO: usize = 2;

// ItemKind::Scripture gains (serde default, skip when empty):
pub also: Vec<AlsoShow>,                // 0..=MAX_ALSO, in display order
pub layout: ParallelLayout,             // SideBySide | Stacked

pub struct AlsoShow { pub translation: String, pub verses: Vec<VerseText> } // verses stored only for offline Bibles

// Settings gains (replaces FEAT-5 BibleSettings):
pub also_show: Vec<String>,             // church default, 0..=MAX_ALSO
pub parallel_layout: ParallelLayout,
```

Rules, applied in one function on add, update and load:
- Keep the first `MAX_ALSO` entries, so a list from a newer build still loads.
- Remove duplicates, the main Bible and blocked Bibles. A blocked Bible is removed with the usual blocked message.

## 3. Three at once: behaviour

**Verse identity.** Library §11's `VerseText` keys (`book`, `chapter`, `verse`) are kept, because labels in other languages can't be parsed reliably. FEAT-5's `verse_id_of` reads those keys and falls back to the label only for old items, where they are 0.

**Lookup.** `bible/lookup` takes `also: Vec<String>` and returns one verse list per Bible. FEAT-5's separate `scripture/parallel` action is dropped. Each extra Bible is mapped through English versification (Library §5.4). A missing verse comes back empty and never shifts.

**Controls.**
- Bible box, Add dialog and Item edit show "Also show: [none ▾]" using the Library §7.2 picker. Once one is picked, a second "Also show" appears. Each has an X. A Side by side / Top and bottom switch appears once any is picked.
- The Bible box controls change the church default, as FEAT-5 decision 8 says. Item edit changes that passage only.
- Under 1500 px window width, the Also show controls fold into the Bible chip's menu.
- Main picked equal to an Also Bible: that Also entry is removed, with the toast "{abbr} turned off (same as main)."
- An Also Bible isn't available: the passage is added with the Bibles that work, plus a toast naming what was left out. The main Bible is never replaced.

**Slides.** FEAT-5's planner stays and takes a list of columns instead of one `second`.
- `Measure::fits(&mut self, main: &str, also: &[ParallelText]) -> Fit`. `compose`, `union` and `split_slide` work over the list.
- The verse sequence is the union across all Bibles. Each column shows only its own text for that verse.
- One font size for all columns; the longest text decides. FEAT-5's `READABLE` and `MIN_PX` are unchanged, so automatic breaking simply uses more slides at three. A fixed verses-per-slide setting can shrink text down to `MIN_PX`.
- A too-long group splits into the same number of parts in every column.
- The operator note lists each missing verse per Bible ("NIV doesn't include Acts 8:37."). This replaces Library §13's "no message" row.

**Drawing.** `SlideContent::Scripture { blocks: Vec<Block>, layout }` with 1 to 3 blocks, replacing Library §11's `a` / `b`. `Block { text, dir, lang, code, credit }`. `SlideLayer::Text.second` becomes `also: Vec<ParallelText>`, and `RenderKey` follows.
- Side by side: equal columns, a 4 % canvas-width gutter and a divider between each pair, a code heading on each column. Main is leftmost; if the main Bible is right-to-left, the column order reverses.
- Stacked: rows in order, a divider between each, a code at the left of each.
- Each block uses its own font and direction. Right-to-left text aligns right in its column.
- Reference line: `John 3:16–17 (KJV · NIV · RVR60)`.
- Credits follow Library §10: shown once per passage, each under its own column or row. FEAT-5 decision 14 (notices on every slide) is replaced.
- Lower-third themes show the main and the first Also Bible, stacked. A third Bible isn't drawn there, and the operator note says so: "Lower-third themes show two Bibles. {abbr} is left out." FEAT-7's Lower third stream view still shows the main only.

**Stage and phone.** Stage shows every block, stacked, as Library §11 says (replaces FEAT-5 decision 13). The remote payload's `text2`, `dir2`, `lang2` become `blocks: [{text, dir, lang}]`.

## 4. Every Bible we can find

**One interface.** `src/bible/library/source.rs`, an enum like today's `Provider` in `sources.rs`, because the project has no `async-trait` crate and one is not needed. Each source lives in its own module and adds one enum variant and one match arm:

```rust
pub enum CatalogSource { Ebible, YouVersion, ApiBible, Door43, Sword }

impl CatalogSource {
    pub fn id(self) -> &'static str;
    pub async fn refresh(self, ctx: &Ctx) -> Result<Vec<CatalogRow>, CatalogError>;
    pub async fn install(self, ctx: &Ctx, row: &CatalogRow) -> Result<Installed, CatalogError>; // download, or add online
}
```

`CatalogRow` can only be built through `CatalogRow::checked(...)`, which runs `blocklist::blocked()` and the licence rule. Rows are private-field structs, so a new source can't skip the blocker. Downloads and imports still run the Library §4 content check.

**Sources**

| Source | Kind | Rule | Status |
|---|---|---|---|
| eBible.org | Offline | Library §3.1 | In Library steps 1–3 |
| YouVersion | Online, church key | Library §3.3. Lists what the key may read, plus "Accept its licence, then Refresh list" | Step 2 |
| API.Bible | Online, church key | Library §3.4 | Step 2 |
| ESV API, NLT API | Online, church key | Existing routes | Done |
| Door43 | Offline | `api.door43.org/v3/catalog.json`, Resource Container zip of USFM. `rights` must be a known open licence. Only `checking_level` 3 is listed | New, step 5 |
| CrossWire SWORD | Offline | Read `DistributionLicense` from each module's `.conf`. Accept only public domain, GPL and Creative Commons. "Copyrighted; Permission to distribute granted to CrossWire" is refused, because that permission doesn't reach us | New, step 5 |
| Bible Brain | Not used | Its licence covers personal, non-commercial use and needs consent for other uses. Add only with written permission | Not now |
| Digital Bible Library | Not used | Needs an agreement | Later |

Door43's catalog shape was read from its public docs and must be checked against a live response when built. SWORD needs a reader for its compressed module format; if that costs more than a small dependency-free module, SWORD ships last.

**The same Bible from two places.** Rows with the same language and normalised title merge into one row. The offline source wins; otherwise the source with a saved key wins. The Library's "Already in your list" rule stays.

**Blocklist.** Unchanged from Library §4: NWT, JST, TPT by abbreviation, title, translated title, notice text and content check. Known YouVersion and API.Bible ids are filled in from live listings at step 2.

**Rate limits.** YouVersion allows about 200 requests an hour per app, and three YouVersion Bibles cost three times as many requests. Lookups use the text cache first. On 429 the app shows the existing "YouVersion is busy. Try again in a minute."

## 5. Online text and Sunday

The Library spec said passages are saved with the service. FEAT-5 §5.1a said online text is never stored in services. FEAT-5's rule wins, because publisher limits (for example ESV's 500 verses) apply to everything an app keeps, and saved services have no cap.

- Offline Bibles: text stored on the item, as today.
- Online Bibles: the item keeps reference and translation. The text goes in the capped per-Bible cache, and verses of the open service are evicted last.
- There is one cache: today's `bible-cache/` with `Provider::cache_policy`. FEAT-5's separate `scripture-cache/` and `TextCache` file are dropped, and its eviction order moves into `bible-cache/`. File names use the Library's id encoding for `yv:` and `ab:` ids.
- Opening a service fetches what's missing.
- **New preflight check:** "{abbr} isn't ready offline for {reference}. Open this service once with internet." It shows in the existing preflight and on the item.
- The Library §3.3.1 Remove message becomes: "Used in N services. Those passages won't show until it's added again."
- Item with no text and no internet: FEAT-5's "Couldn't get NIV text — check the internet." with Try again.

## 6. Tests

- Model: more than 2 Also Bibles trims to 2; duplicate, main and blocked entries removed; old items load with an empty list.
- Planner: union across three Bibles; a verse missing in one or two; same part count in all columns; shared font size; `CharMeasure` and real `Measure`.
- Raster golden images: three columns and three rows; English + Spanish + Hebrew; right-to-left main; credits; lower third with three.
- Payloads: stage and remote with three blocks.
- Catalog: a test source can't build a row for each blocked Bible; Door43 rejects checking level below 3 and unknown licences; SWORD rejects the CrossWire-only permission; two sources merge to one row; offline preferred.
- Preflight: uncached online passage is flagged, cached passage isn't.
- Hand check on Windows: three Bibles on the room screen and stage, read from the back of the room.

## 7. Build order

Start from `v1-candidate`, as `PROJECT_PLAN.md` already suggests. First check `C:\Users\jakes\rp-wt\bible-library`: a Nov 5 status note says it has `src/bible/catalog.rs`, but that file isn't in any commit. Keep that work if it's there, and fit it to `CatalogSource`.

1. Library steps 1–3 as written, with `CatalogSource` used from the start.
2. Merge `feat5-core` and change it from `second` to the `also` list. This is the first code change that touches its tests.
3. Library step 4, built from §2–§3 and §5 here.
4. Door43, then CrossWire SWORD (called "step 5" in §4).

## 8. Open items

- A real YouVersion key is needed to learn which Bibles the church can get. Nothing here is proven against a live key.
- The YouVersion Platform terms are only shown after sign-in. The cache rule must be checked against them then.
- The v1 release gate ("all six translations Ready") still needs Jake's decision; see `docs/superpowers/checks/2026-11-bible-sources.md`.
