# FEAT-6 — Service planning: templates, copy last week, print/export, practice mode

**Status:** Design (November 1, 2026), revised after review. Written while Jake was asleep; every product choice is listed in §11.
**Roadmap:** `PROJECT_PLAN.md` §10b, FEAT-6. **Depends on:** Plan 3 (native operator, `docs/superpowers/plans/2026-11-01-native-operator.md`). Soft links to FEAT-1, FEAT-2, FEAT-3, FEAT-4, FEAT-5, FEAT-7 (§10).
**Controls:** the v1 spec (`2026-10-29-rustypresenter-v1-design.md`) still controls live behaviour. Nothing here weakens §5 (requested/committed, versions, command ids, base), §6.2 (clear matrix), §7.2 (navigation) or §12.3 (phone allowlist).

---

## 1. Purpose and who it's for

Most small churches run nearly the same service every week: walk-in music, a welcome slide, three or four songs, a Bible reading, the sermon slides, the offering slide, one more song, the benediction. Today a volunteer rebuilds that by hand or uses "Copy this service" and then hunts for what to change.

FEAT-6 gives that volunteer four things:

1. **Service templates** — a saved service shape with fixed parts and **blanks** to fill in ("Song", "Bible reading").
2. **Copy last week** — one button makes next week's service from the most recent past one, dated a week later.
3. **Print / save the service order and lyrics** — a run sheet for the pastor and sound desk, a lyric sheet for the band.
4. **Practice mode** — run the service on the laptop (and stage screen) to rehearse while the room screen does not change.

Who: non-technical volunteers on any Windows/macOS/Linux computer, often an old laptop with no projector attached during the week. Wording stays plain, in the tone of `web/operator.js`.

## 2. Scope

**Delivered in three independent parts** (each ships and is tested on its own):

| Part | Contents | Needs |
| --- | --- | --- |
| **FEAT-6a** Planning | service dates, Copy last week, templates, blanks, format guard | Plan 3 |
| **FEAT-6b** Print | Print dialog; HTML through the shared `src/report_page.rs` (open in browser to print, or save). **6b-2** (follow-up): "Save PDF…" via `krilla` | 6a (blanks print) |
| **FEAT-6c** Practice | practice engine, routing, followers, strip | Plan 3; FEAT-2 rules apply when FEAT-2 exists |

**Out:** Planning Center/online plan import (FEAT-7); services created automatically on a schedule; chord charts/keys; rehearsal recording or audio playback in practice; a full-size practice output window; service bundle export (v1 §11); Bible sources and licensing (another session owns `src/bible.rs`); printing slide images (decks print as title + slide count).

## 3. User experience

### 3.1 Services dialog (extends Plan 3 Task 19)

**Top button row:** **Copy last week** (primary), **New from template…**, **New empty service**, **Copy this service**.

**Rows** are sorted by date, newest first; undated services (all made before FEAT-6) come after dated ones in stored order. Each row: the date in the computer's date order (§5.2, e.g. "Sun 11/8/2026" in the US, "Sun 8/11/2026" in the UK) or "No date" in `muted`; the name; the `Open` chip on the current service; "N item(s)"; a `warn` chip "2 blanks" if any. Actions: **Open**, **Rename**, **Date…**, **Print…**, **Save as template**, trash (only with more than one service). Only the 8 newest rows show; **Show N older services** expands the rest while the dialog is open.

**Templates section** below: each row shows name, day ("Sundays", "Any day"), "N parts, M blanks", with **Edit**, **Rename**, trash. Empty: "No templates yet. Use “Save as template” on a service to make one." No built-in template is created.

### 3.2 Copy last week

1. Operator clicks **Copy last week**. The app picks the source (§5.2): the latest service dated today or earlier, else the current service.
2. The **Copy week** overlay (`ui/dialogs/copy-week.slint`): "Make a service for **Sun 11/8/2026** from “Service 11/1/2026”?", checkbox **Leave blanks for songs and Bible readings** (off), a **Change** link on the date (date picker), **Copy** / **Cancel**.
3. If a service already has that date: "There is already a service for Sun 11/8/2026 (“Service 11/8/2026”)." with **Open it** (primary), **Make another**, **Cancel**.
4. **Copy** runs `service/copy-week`; the new service opens; toast "“Service 11/8/2026” is ready" or "… is ready — fill in 4 blanks".

### 3.3 Templates

**Save as template** opens the **Template editor** (`ui/dialogs/template-editor.slint`) prefilled from the service: name = service name with its trailing date removed, day = the service's weekday (Sunday if undated), one row per item. Each row has icon, title and a **Keep** / **Blank** switch: songs and scripture default to **Blank**, everything else **Keep**. A blank row has a label field (default by kind, §5.3) and shows the kind it asks for. Rows move (up/down and drag, same drag code as the service column) and can be removed. **Add a blank** offers Song, Bible reading, Media, Slides, Announcement, Anything. A **Look** row appears once FEAT-1 exists. **Save** runs `template/save`; toast "Template “Sunday Service” saved".

**Edit** reopens the editor; **Rename** prompts "Rename template"; trash confirms "Delete the template “N”? (Services made from it stay.)".

**New from template…**: disabled with tooltip "Save a service as a template first" when there are none; skips the chooser when there is one; otherwise `ui/dialogs/template-pick.slint` lists them. Then a prompt with name (default "`<template name>` `<date>`") and date (next date on the template's day, today counts; Any day → today). **Make service** runs `service/from-template`; toast "“Sunday Service 11/8/2026” is ready — fill in 5 blanks".

### 3.4 Filling blanks

- A blank shows in the service column as a dashed row (`Theme.dash` border, `muted` text): "Song — click to choose".
- Selected, the slides panel shows one large button: **Choose a song** / **Choose a Bible reading** / **Choose media** / **Choose slides** / **Write an announcement** / **Choose what goes here** (Anything → Song tab). It opens the Add dialog in **fill mode**: title "Fill in: Song", every add button reads **Use this**. Choosing runs `service/fill`; the blank is replaced in place; the dialog closes and the filled item is selected.
- Double-click on a blank = the button. **Take Live** on a blank toasts "This is a blank — choose what goes here first."
- Blanks can be removed and reordered like any item.

### 3.5 Service date

**Date…** opens Slint's `DatePickerPopup` in the dialog layer, with **Clear date** (`service/set-date`). **New empty service** and **Copy this service** date the new service today.

### 3.6 Print / save (FEAT-6b)

**Print…** on a service row, or in the slides panel header menu, opens **Print or save** (`ui/dialogs/print.slint`):

- **What:** Service order / Lyrics / Both (default Both).
- **Options:** Include item notes (on), Include Bible readings with the lyrics (on), Start each song on a new page (off), **Paper:** Letter / A4 (default from the computer's region, §5.2).
- **Buttons:** **Print…** (primary), **Save web page…**, **Close**; 6b-2 adds **Save PDF…**.
- **Print…** writes the page with `report_page::write_and_open` into the data folder's `reports/` and opens it in the default browser. Toast "Opened in your web browser — use Print there (you can also save as PDF from the print window)".
- **Save web page…** / **Save PDF…** open a native save dialog (`rfd`), default name "`<service name>` – order and lyrics.html" ("– order" / "– lyrics" when only one is chosen). Toast "Saved `<file name>`".
- Options are remembered in `Settings.print`.

### 3.7 Practice mode (FEAT-6c)

**Start:**

1. Top bar **Practice** button (`TopBtn`, icon `practice`; icon-only when `narrow`), tooltip "Run the service here without changing the room screen". No keyboard shortcut.
2. If the room is **showing something** — `frame.slide || frame.media || frame.background || !frame.overlays.is_empty()` (overlays after FEAT-1) — the **Start practice** overlay (`ui/dialogs/practice.slint`) opens:
   - "The room screen is showing “Amazing Grace” (slide 3). It will stay like that while you practice."
   - **Still running in the room:** one line per running thing, e.g. "Announcement loop", "Countdown", "Music", "Video — at the end it will stop instead of going to the next item". (Lines come from the real engine and FEAT-2 runtime.)
   - Buttons: **Show the logo, then practice** (primary; reads **Clear the screen, then practice** when no logo is set), **Leave it and practice**, **Cancel**. When a countdown or message shows, a note under the first button says "This also hides the countdown." Music is never stopped; when audio plays the overlay says "Music keeps playing."
3. Otherwise (nothing, logo, or audio only) practice starts at once.
4. `practice/start` runs. A strip appears under the top bar in `Theme.practice`: a small **room monitor** (a 160×90 thumbnail of the room's committed picture, Plan 3 `Preview`), "**Practice mode** — the room screen is not changing. Room shows: Logo · Music playing.", buttons **Room: Logo**, **Room: Clear all**, **End practice and show this**, **End practice**.

**While practising:**

- Service column, slides grid, Preview, Take Live, arrows, Space, Enter, thumbnail clicks, Bible box and FEAT-2 controls (messages, timers shown, macros) work as normal but drive the **practice engine**. The Live column header reads **PRACTICE** in `Theme.practice`; live outlines on thumbnails use `Theme.practice`.
- Video and audio slides do not play: the Live monitor shows the item's thumbnail with "Not played in practice"; transport buttons are disabled. Next moves on as usual.
- **F1–F5 act on practice only.** The room changes only through the strip's labelled **Room: Logo** / **Room: Clear all** buttons.
- The stage screen (native and web) follows practice with a **PRACTICE** tag top-right.
- Paired phones follow practice with a banner "Practice — the room screen is not changing". Their buttons (including clears) drive practice; the allowlist is unchanged.
- Real-room notices (room failed, video error) appear as error toasts and in the strip.
- Editing songs, items and the service works as usual; the practice "Up next", titles and phone list update immediately.
- **Forgotten practice guard:** a sticky warning "Practice is still on — the room screen is not changing." with **End practice** appears when the current service's `starts_at` (FEAT-2) is within 15 minutes, or when no practice command has been sent for 60 minutes. **Keep practising** hides it for 30 minutes (the start-time warning returns at the start time itself).

**End:** **End practice** runs `practice/stop`; the strip disappears; Live column, stage and phones show the real room. Nothing is sent to the room. **End practice and show this** runs `practice/stop {take: true}`: it ends practice and then takes the practice slide live in the room with one explicit `Show` command (§5.5). Practice is never saved; the app always starts in normal mode.

## 4. Data model

All new fields have serde defaults. Field-copy rules for every operation are in §5.6.

```rust
// src/model.rs
pub const CURRENT_FORMAT: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlankKind { Song, Scripture, Media, Deck, Text, #[default] Any }

pub enum ItemKind {
    /* existing variants unchanged (FEAT-4 adds Song.arrangement, FEAT-5 adds Scripture.second) */
    /// A part of the service still to be chosen. Shows no slides.
    Blank { #[serde(default)] label: String, #[serde(default)] want: BlankKind },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Day { Sunday, Monday, Tuesday, Wednesday, Thursday, Friday, Saturday }

pub struct Service {
    /* id, name, items */
    #[serde(default, skip_serializing_if = "Option::is_none")] pub date: Option<chrono::NaiveDate>, // "2026-11-08"
    #[serde(default, skip_serializing_if = "Option::is_none")] pub template: Option<Uuid>,          // info only; may dangle
    // FEAT-1 theme, FEAT-2 starts_at: owned by those specs
}
impl Service { pub fn new(name: &str, date: Option<NaiveDate>) -> Self }  // no clock inside

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServiceTemplate {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "sunday")] pub day: Option<Day>,   // None = any day
    pub items: Vec<ServiceItem>,                         // kept items + ItemKind::Blank rows
    // FEAT-1: #[serde(default)] theme; FEAT-2: #[serde(default)] starts_at
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")] pub enum Paper { Letter, A4 }
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")] pub enum PrintWhat { Order, Lyrics, #[default] Both }
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")] pub enum DateOrder { Mdy, Dmy, Ymd }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PrintSettings { pub what: PrintWhat, pub notes: bool /*true*/, pub scripture: bool /*true*/,
                           pub song_per_page: bool /*false*/, pub paper: Option<Paper> /*None = by region*/ }

pub struct Settings { /* existing, #[serde(default)] */ pub print: PrintSettings, pub date_order: Option<DateOrder> /* None = by region */ }
pub struct Data     { /* existing */ #[serde(default)] pub templates: Vec<ServiceTemplate> }
```

- **`Data::default()`** sets `format: CURRENT_FORMAT` and its first service `Service::new(.., None)`. `Store::load` on a missing file builds `Data::default()` and then sets the first service's date to today (the only clock read, in the store/app layer).
- **Format guard (two-step load):** `Store::load` and `Store::restore` first parse `#[derive(Deserialize)] struct Head { #[serde(default)] format: u32 }` (unknown fields ignored). `format > CURRENT_FORMAT` → error "This data was saved by a newer RustyPresenter. Please update the app." and the file is not touched. Only then is `Data` deserialized. `format` 0/1/2 load; every save writes `CURRENT_FORMAT`.
- **Old-build safety:** an older build ignores unknown fields, so it can load a format-2 file without blanks and then save over it, dropping templates and dates. Mitigation: the first time a file loaded as format < 2 is saved, the store first copies `data.json` to `backups/data-before-format-2.json` (once; never overwritten). Documented in the release notes and §11.
- **`referenced_assets`** also walks `templates[*].items[*].background` (and FEAT-1 theme assets), so template backgrounds survive cleanup and are backed up.
- **Delete guards** (`song/delete`, `media/delete`, `deck/delete`) also check templates: "This is in the template “N”. Remove it from the template first." (service messages are checked first).
- `slides::build_item` returns `("<label> (blank)" or "Blank", "blank", vec![])` for a blank.

## 5. Core behaviour

New code: `src/planning.rs` (pure), `src/print/` (pure document + HTML/PDF writers), `src/report_page.rs` (shared with FEAT-7), `src/practice.rs`, new `App::do_action` arms. None of the new actions are on the LAN server.

### 5.1 New actions (`App::do_action`, operator only)

| Action | Body | Result | Errors |
| --- | --- | --- | --- |
| `service/copy-week` | `{source?: Uuid, date?: "YYYY-MM-DD", blanks: bool}` | `{id, name, date, blanks: n}` | "There is no service to copy." |
| `service/from-template` | `{template, name?, date?}` | `{id, name, date, blanks: n}` | "That template is gone." |
| `service/set-date` | `{id, date: String \| null}` | `{}` | "That service is gone.", "That date isn't valid." |
| `service/fill` | `{id, item: ServiceItem}` | `{id}` | "That blank is gone.", "That item is not a blank any more." |
| `template/save` | `ServiceTemplate` (insert/replace by id) | `{id}` | "A template needs a name.", "A template needs at least one part." |
| `template/rename` / `template/delete` | `{id, name}` / `{id}` | `{}` | "That template is gone." / — |
| `print/write` | `{service, what, notes, scripture, song_per_page, paper, format: "html" \| "pdf", path?}` | `{path}` | §8 |
| `practice/start` | `{logo_first: bool}` | `{}` | "Practice is already on." |
| `practice/stop` | `{take?: bool}` | `{shown: bool}` | — (idempotent) |
| `practice/room` | `{what: "logo" \| "clear_all"}` | `Outcome` | "Practice is not on." |

`service/new` (existing) dates the new service today and, with `copy_from`, copies `template`, FEAT-1 theme and FEAT-2 `starts_at`.

`print/write` runs on `spawn_blocking`. With no `path`: `report_page::write_and_open(store, stem, html)` (HTML only). With a path: `write_atomic` to that path. `format: "pdf"` exists only from 6b-2 and requires `path`.

### 5.2 Copy last week (pure: `planning`)

```rust
pub fn week_source(services: &[Service], current: Option<Uuid>, today: NaiveDate) -> Option<Uuid>;
pub fn next_week_date(source: Option<NaiveDate>, today: NaiveDate) -> NaiveDate;
pub fn dated_name(name: &str, old: Option<NaiveDate>, new: NaiveDate, order: DateOrder) -> String;
pub fn format_date(d: NaiveDate, order: DateOrder) -> String;           // 11/8/2026, 8/11/2026, 2026-11-08
pub fn region_defaults(locale: Option<&str>) -> (DateOrder, Paper);
pub fn copy_items(items: &[ServiceItem], blanks: bool) -> (Vec<ServiceItem>, usize);
pub fn find_on_date(services: &[Service], date: NaiveDate) -> Option<Uuid>;
```

- **Source:** the service with the latest `date` that is on or before today; ties go to the later one in `Data.services`. Future-dated services (Christmas Eve planned in November, next Sunday already made) are never the source. None → the current service.
- **Date:** `source.date + 7 days`; if before today, the first same weekday on or after today. Undated source → next Sunday (today if Sunday).
- **Date order and paper** (`region_defaults`): from the OS locale (`sys-locale`) region. Mdy: US, PH, FM, MH, PW; Ymd: CA, CN, JP, KR, TW, HU, LT, MN, IR; otherwise Dmy. Paper Letter for US, CA, MX, PH, CL, CO, VE, else A4. No locale → ISO (Ymd) and Letter. `Settings.date_order` / `print.paper` override.
- **Name:** `dated_name` strips one trailing date token (with a separating space, dash or en dash) **only if** it parses — as M/D/Y, D/M/Y, Y-M-D, 2- or 4-digit years — to exactly `old`. With `old == None` (pre-FEAT-6 services), it strips a token that parses in the computer's order or ISO. Then it appends `format_date(new, order)`. So a UK church's "Service 8/11/2026" with date 2026-11-08 becomes "Service 15/11/2026"; "Sunday Morning" → "Sunday Morning 15/11/2026".
- **Items:** `copy_items` per §5.6.
- The new service is appended, made current, and copies `template`, FEAT-1 theme and FEAT-2 `starts_at` (same time of day).
- The UI checks `find_on_date` first (§3.2); the core does not refuse a duplicate date.

### 5.3 Templates (pure: `planning`)

- `template_from_service(service, keep: &[Uuid], labels: &HashMap<Uuid, String>) -> ServiceTemplate`: kept items copied; others become blanks of the item's kind (Song, Scripture, Media, Deck, Text; Blank stays) per §5.6. Fresh ids.
- `service_from_template(t, name, date)`: items with fresh ids, all fields kept; `template = Some(t.id)`; theme and `starts_at` from the template.
- `default_date(day: Option<Day>, today)`: next date with that weekday, today included; None → today.
- Default labels: Song "Song", Scripture "Bible reading", Media "Media", Deck "Slides", Text "Announcement", Any "Something".

### 5.4 Blanks and live

- A blank builds to zero slides; `Live::take_index` already skips such items, so **Next/Prev step over blanks**; `next_preview` (foldback and phone "next") uses the same rule.
- `Show` on a blank (`LiveItem.kind == "blank"`) returns "This is a blank — choose what goes here first." instead of "That item has no slides."
- `remote_view` lists blanks greyed with "(blank)" and no tap-to-take.
- Filling a blank is a data edit; it never changes the room (v1 §7.2 snapshot rule).
- `service/fill` checks the target is still `ItemKind::Blank` (two quick fills cannot overwrite a real item); field rules in §5.6.

### 5.5 Practice engine (`src/practice.rs`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineId { Real, Practice }

pub struct Practice { pub live: Live, pub started: Instant, pub last_command: Instant }
pub const PRACTICE_VERSION_GAP: u64 = 1_000_000_000;   // < 2^53, safe in phone JavaScript
impl Live { pub fn starting_at(version: u64) -> Live }  // `new()` = starting_at(0)

// App
pub practice: Mutex<Option<Practice>>,   // lock order: data → live → practice → automation (FEAT-2)
pub practice_floor: AtomicU64,           // first version the next session may use
pub fn practice_active(&self) -> bool;
pub fn practice_view(&self) -> Option<LiveView>;   // practice=true, room=Some(summary of real committed frame)
pub fn display_view(&self) -> LiveView;            // practice view if on, else live_view()
pub fn with_engine<T>(&self, id: EngineId, f: impl FnOnce(&mut Live) -> T) -> Result<T, String>; // Practice when off → "Practice has ended."
```

**Versions.** Start: `Live::starting_at(max(real.version + GAP, practice_floor))`. Stop: `practice_floor = practice.live.version + 1`. A base from one engine, or from an earlier practice session, is always stale in the current one.

**Updates.** `Update` gains `Practice(Option<LiveUpdate>)` (`None` = practice ended). `Update::Live` keeps carrying only the real engine. `LiveView` gains `practice: bool` (`skip_serializing_if = "Not::not"`, so today's JSON is byte-for-byte unchanged) and `room: Option<String>` (set only on practice views). `broadcast_live()` sends `Update::Live(real)` and then, when practice is on, `Update::Practice(Some(practice_view))` — so every data edit (`App::edit`), `room_event` and `room_recovered` refreshes practice's "Up next", titles, phone list and the strip's room text, and followers end on the practice view.

**Command routing while practice is on** (`App::command`; `env.cmd` from operator or phone):

| Command | Goes to | Notes |
| --- | --- | --- |
| `Next`, `Prev`, `Show` | practice | base checked against practice version |
| `ClearAll`, `ClearSlide`, `ClearMedia`, `ClearAudio`, `Logo` (F1–F5, phone) | practice only | room clears use `practice/room` |
| `MediaPause`, `AudioPause` | practice | "Not played in practice." |
| FEAT-2 `ShowMessage`, `HideMessage`, `ShowTimer`, `HideTimer` | practice only | overlays live in the practice frame |
| FEAT-2 `RunMacro` | practice only | `apply_batch` on practice; media/audio steps go into the practice frame unplayed; any future step that targets the room by name is refused: "`<macro>`: step N — not available in practice." |
| FEAT-2 `SetAutoAdvance` | the engine on screen (practice) | — |
| FEAT-2 internal `AutoNext`, `AutoHideMessage` | the `EngineId` stored in their run state | dropped if that engine has ended |
| FEAT-2 `TimerAtZero` | each engine whose frame shows that timer (id, `id#practice`) | timers are global |
| `practice/room` | real engine, fresh id `room-<uuid>` | explicit, labelled |
| `RoomEvent` (`App::room_event`) | real engine | — |

After every accepted practice command the core calls `practice.live.report(RoomEvent::Committed { version })` (no room, so requested = committed; `pending` never shows) and sets `last_command`.

**Automation is bound to its engine.** FEAT-2's `AutoRun` and `ShownMessage` gain `engine: EngineId`; the runtime holds at most one `AutoRun` per engine. A loop or auto-advance started on the room before practice keeps running on the real engine during practice (the room keeps cycling); one started during practice runs on practice. Nothing moves between engines on start or stop; stop drops practice-bound runs. The ticker calls `with_engine(run.engine, …)`.

**Real video end action during practice.** While practice is on, a real `EndAction::Next` is treated as `Stop` (the room never advances unseen, and never into another service after a service switch). The start overlay says so (§3.7).

**Start (`practice/start`).** If `logo_first`: apply `Command::Logo` to the real engine (id `practice-logo-<uuid>`); under FEAT-2 rules this also hides room overlays such as the countdown. Then create the practice engine with nothing live and broadcast `Update::Practice(Some(view))`.

**Stop (`practice/stop`).** Remember the practice cursor `(item_id, slide)`; drop the engine; set `practice_floor`; broadcast `Practice(None)` then `Live(real)`. With `take: true` and a cursor whose item is still in the current service, apply `Command::Show { item, slide }` to the real engine with `base = real.version` and id `practice-take-<uuid>`; result `shown: true`. Otherwise `shown: false` and toast "That slide isn't in the service any more."

**Room suppression is structural:**

- `ui::room` (native) matches only `Update::Live`; on `Lagged` it reads `live_view()`.
- `server::event_stream` gets a **per-role map**: `room` → `Update::Live` JSON, `Update::Practice` skipped, Lagged → `live_view()`. `stage`/`operator` → `Practice(Some)` JSON; `Practice(None)` skipped (the real `Live` follows); `Live` skipped while `practice_active()`; Lagged and first message → `display_view()`.
- `remote_view(app)` locks data → live → practice and reads the **active engine** (practice `Live` when on, else `app.live`) for `version`, `cursor`, `current`, `next`, `on_air`, `media`, `notice`; it adds `"practice": true` and `"room": "<summary>"` (both omitted when off). `remote_map` already sends a fresh `remote_view` for every non-job update and on Lagged; this now explicitly includes `Update::Practice`.
- The practice engine never produces a `room::Plan`.

**Followers:**

| Follower | Practice on | Practice off |
| --- | --- | --- |
| Room (native `ui::room`, web `output.html`) | real only | real |
| Livestream (FEAT-7) | real room's committed picture, like the room | real |
| Stage (native `ui::stage`, web `stage.html`) | practice view + PRACTICE tag; first `show()` and Lagged use `display_view()` | real |
| Operator | Live column = practice; strip = room monitor + `room` text; real notices → toasts | real |
| Phone remote | `remote_view` of practice engine, `practice: true` → banner | real |

**Other interactions.** FEAT-2 timers keep ticking. FEAT-7 usage log records only real-engine commits. Service switch during practice: practice continues with the new order; the real engine is unaffected (and cannot advance, per the end-action rule). A live item edited during practice keeps its snapshot in both engines (v1 §7.2).

### 5.6 What each copy keeps

| Field | Copy last week / Copy this service | Template from service (kept / blanked row) | Service from template | Fill a blank |
| --- | --- | --- | --- | --- |
| `id` | new | new / new | new | blank's id |
| `kind` (incl. FEAT-4 `arrangement`, FEAT-5 `second`) | kept; with blanks on, Song/Scripture → Blank | kept / Blank | kept | chosen |
| `notes` | kept | kept / kept | kept | blank's, then chosen's after a blank line |
| `background` | kept | kept / kept | kept | `chosen.or(blank)` |
| FEAT-2 `advance` | kept | kept / kept | kept | `chosen.or(blank)` |
| FEAT-3 `stage_layout` | kept | kept / kept | kept | `chosen.or(blank)` |
| FEAT-3 `slide_notes` | kept; dropped when blanked | kept / dropped | kept | chosen's |
| Service `date`, `template` | new date; source's template | — | given date; template id | — |
| Service FEAT-1 `theme`, FEAT-2 `starts_at` | kept (same time of day) | → template | from template | — |

### 5.7 Edge cases

- Only undated services: source = current service, date = next Sunday. Only future-dated services: source = current service.
- Template with zero kept items, or a blank-only service saved as a template: allowed (at least one row).
- Deleting a template leaves `Service.template` dangling (information only).
- Filling a blank removed meanwhile: "That blank is gone."; the dialog stays open.
- Print: blanks print as "Blank — Song (not chosen yet)"; a song with no lyrics prints its title with "(no lyrics)".
- Practice with no room window: strip "Room shows: nothing (no room screen)", monitor black.
- Crash during practice: restart in normal mode; the room recovers its real committed layers (v1 §4.2).
- "Leave it and practice" with a real video: it keeps playing; at its end it stops (never Next) while practice is on.

## 6. Rendering

### 6.1 Print documents (`src/print/`)

```
src/print/mod.rs   // pub fn html(data, req) -> String;  (6b-2) pub fn pdf(data, req, fonts) -> Result<Vec<u8>, String>
src/print/doc.rs   // PrintDoc builders (pure)
src/print/html.rs  // PrintDoc → self-contained HTML
src/print/pdf.rs   // (6b-2) PrintDoc → PDF via cosmic-text + krilla
src/print/text.rs  // every user-visible print string
src/report_page.rs // shared with FEAT-7: write_and_open(store, stem, html) -> Result<PathBuf, String>
```

```rust
pub struct PrintDoc { pub title: String, pub subtitle: String, pub blocks: Vec<Block> }
pub enum Block { Heading(String), Credit(String),
    OrderRow { n: usize, kind: String, title: String, detail: String, notes: String, blank: bool },
    Order(String), Section { label: String, lines: Vec<String> }, Para(String), PageBreak }
```

`report_page::write_and_open` writes `<data>/reports/<stem>-<YYYYMMDD-HHMMSS>.html` atomically and opens it with `opener::open`. At startup the store deletes `reports/*.html` older than 30 days. FEAT-7 uses the same helper; whichever lands first builds it.

### 6.2 Service order

Header: service name; date in long form for the region ("Sunday, November 8, 2026" via `format_date_long`); "CCLI License # 123456" when `settings.church_ccli` is set. One numbered `OrderRow` per item:

| Kind | Title | Detail |
| --- | --- | --- |
| Song | title | writers; "Order: V1 C V2 C" from the item's `arrangement`, else the song's |
| Scripture | reference | translation code; "+ `<second code>`" when FEAT-5 `second` is set |
| Media | name | "Video"/"Picture"/"Audio"; "(loops)" |
| Deck | title | "N slides" |
| Text | title | first body line, ≤ 80 characters |
| Blank | "Blank — Song" | "(not chosen yet)", italic grey |

With **notes** on, item notes print under the row in italic. FEAT-3 `slide_notes` are never printed. Footer (PDF; HTML via `@page` where the browser supports it): "Printed from RustyPresenter · `<date>`" and "Page n of m".

### 6.3 Lyrics

For each song item in service order: `Heading(title)`, `Credit` lines (writers; "© …"; "CCLI Song # x"), `Order(…)` from the item's `arrangement` override, else the song's arrangement, else natural order (labels joined " · "), then each distinct section once in first-appearance order (`slides::parse_sections` + `slides::song_order`), lines as written. With **scripture** on, each scripture item adds `Heading(reference + " (" + code + ")")`, its verses as one paragraph with verse numbers (via `bible::source(code)?.passage(&r)`), and, when FEAT-5 `second: Option<SecondText>` is set, a second paragraph headed with `second.translation`. A copyright line prints only when `TranslationSource::copyright()` exists (cross-spec request to FEAT-5: `fn copyright(&self) -> &str { "" }` default method); until then the code alone is printed. **Song per page** inserts `PageBreak` before each song after the first. Blanks are skipped. No songs or readings → "No songs or Bible readings in this service." **Both** = order, `PageBreak`, lyrics.

### 6.4 HTML (6b)

One UTF-8 file, inline `<style>` with `@page { size: letter|A4; margin: .75in }`, black on white, `"Segoe UI", "Noto Sans", sans-serif` with the browser's own font fallback (Korean etc. work wherever the browser can show them), `PageBreak` → `break-before: page`, sections `break-inside: avoid`. Every value HTML-escaped. No scripts, no remote resources.

### 6.5 PDF (6b-2)

cosmic-text `Buffer`s from the shared `render::Fonts` at column width (body 11 pt, headings 15 pt semibold, credits 8.5 pt, margins 0.75 in / 19 mm); a section never splits if it fits on one page; each glyph run drawn with the face cosmic-text chose, so `krilla` embeds and subsets only used fonts. Letter 8.5 × 11 in, A4 210 × 297 mm. Title metadata = service name; no other metadata.

### 6.6 Slides in practice

The operator Live monitor (`src/ui/preview.rs`) and stage render the practice `Frame` with the same code as real frames (FEAT-1 overlays included). For video/still/audio media in practice, `Preview` draws the Plan 3 thumbnail and "Not played in practice"; no mpv player is created. The strip's room monitor re-renders only on `Update::Live`, at 160×90.

## 7. UI (extends Plan 3)

**New `.slint` files** (colours from `Theme`; labels in `@tr(...)`):

- `ui/dialogs/copy-week.slint` — `CopyWeek`: `date-text`, `source-name`, `existing-name`, `in-out blanks`; callbacks `copy`, `open-existing`, `make-another`, `change-date`, `cancel`.
- `ui/dialogs/template-editor.slint` — `TemplateEditor` (wide `DialogShell`): name, day `ComboBox`, rows `[TemplateRow { icon, title, keep, label, want }]`; callbacks `toggle`, `label-edited`, `move`, `remove`, `add-blank`, `save`.
- `ui/dialogs/template-pick.slint` — chooser for **New from template…**.
- `ui/dialogs/print.slint` — `PrintDialog`: what `Segmented`, three `CheckBox`es, paper `Segmented`, buttons; `in property <bool> busy`, `in property <bool> pdf` (6b-2 shows Save PDF).
- `ui/dialogs/practice.slint` — `StartPractice`: `room-text`, `[string] running`, `has-logo`, `hides-countdown`, `music`; three buttons.
- `ui/operator/practice-strip.slint` — `PracticeStrip { in property <image> room; in property <string> room-text; in property <bool> warn; callback room-logo(); callback room-clear(); callback end(); callback end-and-show(); callback keep(); }`.

**Changed:** `ui/theme.slint` (`practice: #4aa3ff`, `practice-ink: #06121f`, `practice-strip-bg: #4aa3ff26`); `ui/icons.slint` (`practice`, `template`, `print`, `calendar`, `blank`); `ui/operator.slint` (`practice`, strip under top bar); `ui/operator/topbar.slint` (Practice `TopBtn`); `ui/operator/service.slint` (blank rows, "N blanks"); `ui/operator/slides.slint` (Choose… button, Print…); `ui/operator/live.slint` (PRACTICE header/outline, caption); `ui/dialogs/services.slint` + `src/ui/dialogs/services.rs` (§3.1); `ui/dialogs/add.slint`, `src/ui/dialogs/add.rs`, `add_media.rs` (`fill: Option<Uuid>` mode); `ui/stage.slint` + `src/ui/stage.rs` (PRACTICE tag; `display_view()` on first show and Lagged; handles `Update::Practice`); `src/server.rs` (per-role map, `remote_view`); `web/stage.html` (PRACTICE tag from `view.practice`); `web/output.html` (no change in behaviour; covered by the room map test); `web/remote.html`/`remote.js` (banner from `practice`, room text, blanks greyed).

**New Rust glue:** `src/ui/dialogs/copy_week.rs`, `template_editor.rs`, `print.rs`, `practice.rs`; `src/ui/operator/practice.rs` (strip, room monitor, warning timer). `DialogKind` gains `TemplateEditor(TemplateDraft)` and `Print(Uuid)`; copy-week, template-pick and practice are ask-level overlays (`OpState.asks`).

**Operator follower (Plan 3 Task 5):** `Update::Practice` → `PracticeSnap`. `OpState` gains `practice: Option<LiveUpdate>`; `apply_live` draws practice when `Some`, else real; the room monitor draws the real view. Bases come from the drawn view. On Lagged it reads both `live_view()` and `practice_view()`.

**Keys:** no new shortcuts. In practice F1–F5 act on practice (§5.5).

**Strings:** `src/planning_text.rs` (services, templates, practice, toasts, core errors) and `src/print/text.rs`. Plain wording ("blank", "practice").

**Cargo.toml changes** (exact versions pinned at implementation to a release at least two weeks old; licences checked against MIT OR Apache-2.0):

| Part | Crate | Change | Licence |
| --- | --- | --- | --- |
| 6a | `chrono` | add feature `serde` | MIT OR Apache-2.0 |
| 6a | `sys-locale` | new (region for date order/paper) | MIT OR Apache-2.0 |
| 6b | `opener` | new (open file in browser/viewer) | MIT OR Apache-2.0 |
| 6b | `rfd` | reuse Plan 3's; add if absent | MIT |
| 6b-2 | `krilla` | new | MIT OR Apache-2.0 |

## 8. Errors

| Situation | What the operator sees |
| --- | --- |
| Copy last week with no services | Button disabled (defensive "There is no service to copy.") |
| Template save, empty name | Field in `danger`, "A template needs a name." |
| Fill a blank that changed | Toast "That item is not a blank any more."; dialog stays |
| Invalid date | "That date isn't valid." |
| Report page write fails | "Couldn't make the page: `<reason>`." |
| No browser/viewer opened it | "The page is saved at `<path>` but no program opened it. Open it from there to print." + `CopyText` |
| Save path not writable | "Couldn't save to that folder: `<os error>`." |
| PDF fails (6b-2) | Toast (6 s) "Couldn't make the PDF: `<reason>`. Try “Save web page…” instead." |
| `practice/start` while on | "Practice is already on." |
| End and show, item gone | "That slide isn't in the service any more." (practice still ends) |
| Phone command stale across start/stop | Normal stale handling; phone resyncs |
| Data from a newer app | Startup error "This data was saved by a newer RustyPresenter. Please update the app."; file untouched (also on restore) |

Print runs off the UI thread and never affects output. Practice has no room path.

## 9. Testing

### 9.1 Core unit tests

`src/planning.rs` (6a): `week_source_prefers_latest_past_date`, `week_source_ignores_future_dated_services`, `week_source_future_only_falls_back_to_current`, `week_source_ties_take_later_service`, `week_source_falls_back_to_current_when_undated`, `next_week_adds_seven_days`, `next_week_skips_to_future_same_weekday`, `next_week_undated_is_next_sunday`, `next_week_on_sunday_undated_is_today`, `dated_name_replaces_us_date`, `dated_name_replaces_uk_date_matching_old`, `dated_name_keeps_non_matching_date_text`, `dated_name_replaces_iso_date`, `dated_name_appends_when_no_date`, `dated_name_strips_en_dash`, `region_defaults_us_gb_jp_unknown`, `copy_items_new_ids_keep_all_fields`, `copy_items_blanks_keep_notes_background_advance_layout_drop_slide_notes`, `copy_items_keeps_existing_blanks`, `template_from_service_keep_and_blank`, `template_blank_labels_default_by_kind`, `service_from_template_fresh_ids_and_template_id`, `default_date_next_weekday_includes_today`, `default_date_any_day_is_today`.

`src/model.rs` / `src/store.rs` (6a): `old_data_json_loads_with_no_templates_and_no_dates`, `blank_item_round_trips`, `service_date_serializes_iso`, `print_settings_default`, `data_default_is_current_format`, `format_3_is_refused_and_file_untouched`, `restore_refuses_newer_format`, `first_format_2_save_copies_old_file_once`, `referenced_assets_include_template_backgrounds`, `media_delete_blocked_by_template`, `song_delete_blocked_by_template`, `deck_delete_blocked_by_template`.

`src/live.rs` / `src/app.rs` (6a): `next_skips_blank_items`, `prev_skips_blank_items`, `next_preview_skips_blanks`, `show_blank_says_choose_first`, `fill_replaces_blank_in_place_keeps_id_and_notes`, `fill_keeps_blank_background_when_choice_has_none`, `fill_refuses_non_blank`, `copy_week_action_selects_new_service_and_reports_blanks`, `copy_week_copies_starts_at_and_theme`, `from_template_action_uses_template_day`.

`src/practice.rs` / `src/app.rs` (6c): `practice_versions_start_above_gap`, `second_practice_session_rejects_first_session_base`, `practice_next_does_not_change_real_engine`, `practice_commands_commit_immediately`, `practice_f1_does_not_touch_room`, `practice_f2_to_f5_do_not_touch_room`, `practice_room_logo_changes_real_engine_only`, `practice_resend_same_id_is_duplicate`, `real_base_is_stale_in_practice`, `practice_base_is_stale_after_stop`, `start_with_logo_first_puts_logo_on_room`, `stop_broadcasts_practice_none_then_real_live`, `stop_take_shows_practice_slide_on_room`, `stop_take_missing_item_only_stops`, `room_event_during_practice_goes_to_real_engine`, `edit_during_practice_rebroadcasts_practice_view`, `media_pause_in_practice_errors`, `real_loop_keeps_running_on_room_during_practice`, `practice_auto_advance_dropped_on_stop`, `message_in_practice_stays_in_practice_frame`, `macro_in_practice_runs_on_practice_only`, `real_video_end_next_during_practice_does_not_follow_service_switch`, `live_view_json_unchanged_when_not_practising`, `display_view_is_practice_while_on`.

`src/server.rs` (6c): `web_room_stream_ignores_practice`, `web_stage_stream_follows_practice_with_flag`, `remote_view_uses_practice_engine_and_version`, `remote_json_has_practice_and_room_only_while_on`, `phone_next_during_practice_advances_practice_not_room`, `remote_allowlist_unchanged`.

`src/ui/stage.rs` (6c): `stage_lagged_resync_uses_display_view`.

`src/print/` (6b): `order_rows_for_every_kind`, `order_uses_item_arrangement_override`, `order_marks_blanks`, `order_includes_notes_only_when_asked`, `order_never_prints_slide_notes`, `lyrics_print_each_section_once_with_order_line`, `lyrics_print_second_translation`, `lyrics_skip_blanks`, `lyrics_scripture_optional`, `lyrics_song_per_page_inserts_breaks`, `lyrics_empty_service_message`, `html_escapes_text`, `html_has_no_script_or_remote_urls`, `html_page_size_from_paper`, `report_page_writes_to_reports_and_cleans_old`. 6b-2: `pdf_has_page_count`, `pdf_letter_and_a4_media_boxes`, `pdf_embeds_fixture_font_for_hangul` (a small OFL Hangul subset font in `tests/fixtures/fonts/` loaded into `Fonts`; no system font needed).

### 9.2 UI tests (Slint testing backend, `Ops::inline`, `tests/ui_operator_flows.rs` style)

6a: `copy_last_week_creates_dated_service_and_opens_it`, `copy_last_week_existing_date_offers_open`, `copy_last_week_blanks_checkbox_makes_blanks`, `services_sorted_by_date_undated_last`, `older_services_collapse_after_eight`, `save_as_template_defaults_songs_to_blank`, `template_editor_add_blank_and_reorder`, `new_from_template_disabled_without_templates`, `blank_row_shows_choose_button_and_opens_add_in_fill_mode`, `fill_mode_use_this_replaces_blank`, `enter_on_blank_toasts_choose_first`.
6b: `print_dialog_remembers_options`, `print_save_html_writes_file` (`Ops` receives the path directly).
6c: `practice_button_starts_without_prompt_when_room_empty`, `practice_prompt_when_room_showing_slide`, `practice_prompt_lists_running_loop`, `practice_prompt_says_clear_when_no_logo`, `practice_strip_shows_room_text_and_end_button`, `practice_strip_room_logo_button`, `practice_live_header_says_practice`, `practice_f1_clears_practice_only`, `practice_warning_after_idle_hour`, `end_practice_and_show_this_takes_slide_live`, `practice_video_slide_shows_not_played_caption`, `end_practice_restores_real_live_view`, `stage_shows_practice_tag`.

### 9.3 Manual checks (church laptop + an old laptop; Windows, macOS, Linux once each)

1. Copy last week on real data with a future service present; names/dates right; undated services still listed. Repeat with the OS region set to UK.
2. Template from last Sunday → next Sunday → fill every blank.
3. Print Both on Letter and A4 from Edge/Safari/Firefox; page breaks sane; a Spanish song prints. A Korean song prints where the computer has a Korean font (skip otherwise).
4. Practice with the projector on and the walk-in loop running: room keeps looping; F1 in practice leaves the room alone; **Room: Logo** changes it; stage shows PRACTICE; a phone shows the banner, its Next and Clear drive practice only.
5. End practice and show this: the practised slide appears in the room. End practice: room unchanged, phone resyncs.
6. Kill the app during practice; restart in normal mode with the real room content.
7. Old build opened on a FEAT-6 data file (developer check): `backups/data-before-format-2.json` exists.

## 10. Dependencies on other FEAT specs

- **Plan 3 (required):** Services and Add dialogs, DialogShell/ask overlays, `Ops`, `Theme`, `Thumbs`, `Preview`, follower thread.
- **FEAT-1:** `Service.theme`/`ServiceTemplate.theme` copied per §5.6; template editor Look row; practice frames render through the overlay layer; "showing something" includes overlays. Not required to ship.
- **FEAT-2:** copy rules for `starts_at` and `advance` (§5.6); **requests to FEAT-2:** add `engine: EngineId` to `AutoRun` and `ShownMessage`, hold one `AutoRun` per engine, route through `App::with_engine`, and extend its lock order to data → live → practice → automation; the routing table in §5.5 is the contract for its commands in practice. `practice/*` are not macro steps. `starts_at` feeds the forgotten-practice warning. FEAT-2 uses `jiff` for timestamps; FEAT-6 dates stay `chrono::NaiveDate` (already a dependency); both serialize as "YYYY-MM-DD", so a later switch is a type change only.
- **FEAT-3:** stage follows `display_view()` and keeps a PRACTICE tag in every layout. **Request:** `stage/layout/delete` also clears `templates[*].items[*].stage_layout`. `slide_notes` are not printed and are dropped when blanking.
- **FEAT-4:** print uses `ItemKind::Song.arrangement`, then the song's arrangement; section colours not printed.
- **FEAT-5:** print uses `bible::source(code)`, `TranslationSource::passage`, `SecondText`; **request:** a default method `fn copyright(&self) -> &str { "" }` on `TranslationSource`.
- **FEAT-7:** **requests:** the livestream follows the room's committed picture during practice (drop `stream_blank_in_practice_mode`; replace with `stream_ignores_practice_engine`); the usage log ignores practice-engine commits (real room commits during practice still count). `src/report_page.rs` is shared (`reports/`, 30-day cleanup). Planning Center import creates dated `Service`s and may use `ItemKind::Blank` for unmatched songs.

## 11. Decisions for Jake to review

| # | Decision (default chosen) | Main alternative |
| --- | --- | --- |
| 1 | **Copy last week keeps everything**; a checkbox (off) turns songs and readings into blanks | Blanks by default |
| 2 | Source of "last week" = the **latest service dated today or earlier** | The service that is open now |
| 3 | If the copied date is in the past, jump to the **next same weekday from today** | Always exactly +7 days |
| 4 | New services are **dated**; old services stay undated ("No date") | Guess dates from their names |
| 5 | Only the **8 newest services** show; older behind "Show N older" | Show all; or auto-archive after 3 months |
| 6 | **No built-in starter template** | Ship a "Typical Sunday" template |
| 7 | Saving a template, **songs and Bible readings default to blanks** | Everything kept |
| 8 | Template day defaults to **Sunday** | Any day |
| 9 | Unfilled items are **"blanks"**, **skipped** by Next/Prev | "Placeholders"; or Next stops on them |
| 10 | **Date format and paper follow the computer's region** (US: 11/8/2026 + Letter; UK/most: 8/11/2026 + A4; unknown: 2026-11-08 + Letter); overridable in Settings | Always US format and Letter |
| 11 | Print defaults: **Both**, notes on, readings on, songs not on separate pages | Order only |
| 12 | **HTML first**: Print opens a page in the browser (print or save as PDF there); in-app "Save PDF" (krilla) is a later follow-up (6b-2) | In-app PDF from the start; or OS print dialog |
| 13 | Lyrics print **each section once with an "Order:" line** | Full performance order with repeats |
| 14 | Print sheets **black and white**; FEAT-3 slide notes **not printed** | Colour sheets; print slide notes |
| 15 | Practice start: **room keeps what it shows**; if it shows something, ask "Show the logo, then practice" (default) / "Leave it", listing what still runs in the room | Always clear the room to the logo |
| 16 | Practice: **F1–F5 and phone clears act on practice only**; the room has its own **Room: Logo / Room: Clear all** buttons in the strip | F1 also clears the room as an emergency key |
| 17 | Practice: **stage follows practice** with a PRACTICE tag | Stage keeps the real room |
| 18 | Practice: **phones follow and drive practice** with a banner | Phones locked out |
| 19 | Practice: **videos/audio do not play** (thumbnail + "Not played in practice") | Play on the laptop's speakers |
| 20 | Practice: a room **video set to "Next" at its end stops instead** while practising | Let it advance the room unseen |
| 21 | Practice: **room automation keeps running** (walk-in loop keeps cycling); automation started in practice stays in practice | Pause room automation during practice |
| 22 | Practice: **forgotten-practice warning** 15 min before service start or after 60 idle minutes; **End practice and show this** button | No warning; end only |
| 23 | Practice: **livestream follows the real room**, not practice (request to FEAT-7) | Stream blank in practice (FEAT-7's earlier choice) |
| 24 | Practice is **never remembered**; app always starts normal; not startable from phone or macro | Remember across restarts; allow in macros |
| 25 | Practice colour **blue** (`#4aa3ff`); no keyboard shortcut | Purple/grey; Ctrl+Shift+P |
| 26 | Old-build safety: **copy of the old data file** on first format-2 save | Full backup with assets; or nothing |
| 27 | Planning Center/online planning stays in FEAT-7; FEAT-6 is local only | — |

## Review notes

- Locale date order: used the OS locale region (`sys-locale`) plus a small region table instead of reading each OS's short-date pattern, because the pattern APIs differ per OS and Linux has no reliable one; a Settings override covers the rest. Fallback is ISO as asked.
- F1 as an emergency key that also clears the room (optional in the finding) was not adopted: one key with two effects is what the finding warned against; the strip's room buttons are one click away. Kept as the alternative in Decision 16.
- Old-build mitigation copies only `data.json` (assets are content-addressed and are not removed by the format change), which is cheap on old laptops.
