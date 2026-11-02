# FEAT-1 — Themes, Backgrounds and Overlays: Design

Date: November 1, 2026 · Status: design written by Claude while Jake was away (revised after review); product defaults are listed at the end for review.
Roadmap: `PROJECT_PLAN.md` §10b, FEAT-1. Builds on the v1 spec (`2026-10-29-rustypresenter-v1-design.md` §5–§7, §12), the native spec (`2026-10-29-native-slint-design.md` §3, §4, §6) and Plan 3 (`docs/superpowers/plans/2026-11-01-native-operator.md`). Which layers each output shows ("screen views") is FEAT-7, not this spec.

## 1. Purpose and who it's for

A church's slides should look the same every Sunday without anyone thinking about it, and changing the look (Christmas, Easter, a youth night, a new projector) should take minutes, not an afternoon. Today there is exactly one style (`Settings.theme` + `Settings.background`) and one place to change it (Settings → Look). That is fine for the church RustyPresenter started at, but most churches need at least a second style for a special service, and announcements need more than plain text.

Who uses it:
- **The Sunday volunteer** never opens a theme editor. Slides come out in the church's look. At most they toggle an overlay (the church logo in the corner, a clock, "Wi-Fi: GraceGuest") from the Live column or the phone.
- **The person who "does the screens"** (often the same volunteer, a few times a year) makes a "Christmas" theme once, picks it for the Christmas Eve service (or makes it the church default for December), and is done. In January they star "Standard" again.
- **Whoever makes the announcements** wants a word in bold, a date in gold, a bigger heading — without learning anything.

FEAT-1 delivers four things:
1. **Named themes.** "Standard" (today's style) plus any number of saved themes; one of them is starred as the **church default**. A service or a single item can use a saved theme. Resolution: **item > service > church default**.
2. **Rich text on announcement slides and text overlays**: bold, italic, underline, colour and size for any part of the text.
3. **A background library**: built-in colours and gradients, plus the church's own pictures and looping videos, with a "darken" setting so text stays readable.
4. **The overlay layer**: a generic layer above the slide text made of keyed **slots**. FEAT-1 fills slots from saved overlays (text, picture, clock); FEAT-2 fills them with timers and messages; FEAT-7 decides per output whether the layer shows. One layer, one drawing function, one set of clear rules.

## 2. Scope

**In**
- `SavedTheme` (name + text style + optional background), CRUD actions, the starred church default (a reference), the "Standard" entry, a read-only built-in theme registry, per-service and per-item theme references, resolution, snapshot at Take Live (a later theme difference on the live item is reported through FEAT-4's change banner, §5.3).
- The single Settings tab table and the single keyboard-shortcut table for all FEAT specs (§7.5, §7.6), and the operator layout budget at 1366×768 (§7.2).
- The data-format constant `store::SUPPORTED_FORMAT`, the migration chain and the one load hook `Data::after_load` (§4.7).
- The clear epoch (`Live.clear_epoch`, `seen_clear`) that lets a clear always win over a delayed show without a base (§5.2).
- New text-style options: weight, alignment, top position, outline, text band (a box behind the text).
- Bundled fonts that look the same on every computer (§7.4), including Noto Sans Bold and Italic (required by rich text).
- Rich text (`StyleSpan` runs) for announcement (`ItemKind::Text`) bodies and text overlays; a small rich-text editor widget.
- Background library: `Background::Gradient`, a `dim` value on image/video/gradient backgrounds, built-in backgrounds, library entries (with video conversion status), the reworked background picker, a Backgrounds manager dialog.
- The overlay layer: `Frame.overlays: Vec<OverlaySlot>`, `OverlaySource` (FEAT-1 arms `Text`, `Image`, `Clock`), private `show_overlay`/`hide_overlay` mutators, the per-source Logo policy, saved overlays, live commands `Overlay { id, on }` and `ClearOverlays` (F6), non-fatal overlay preparation, drawing in `src/render/overlay.rs`, room window, operator live preview, phone toggles.
- A `settings/logo` action (the Logo section moves to the Themes tab).
- Data migration (format 1 → 2), a forward-compatibility guard for newer data, labelled backups, asset references, strings module, the renderer's `CacheKey` (§6.4).

**Out** (with where it goes)
- Which screen shows which layers, lower-third/livestream output: FEAT-7.
- Timers, messages with fields, macros, auto-advance, the pre-service countdown: FEAT-2 (they are sources on this overlay layer).
- Stage layouts: FEAT-3. The stage keeps showing plain text and does not show overlays.
- Song-section colours: FEAT-4. Per-song default theme: later (see Decisions).
- Rich text in song lyrics or scripture (both stay theme-only, plain text).
- Per-paragraph alignment, free text boxes (a slide designer), transitions other than the existing 400 ms fade.
- Sharing a theme with another church as a file: later. Bundled photo or motion backgrounds (see Decisions).
- Automatic asset cleanup (none exists today; §4.9 states the rule any future cleanup must follow).

## 3. User experience

### 3.1 Themes (Settings → Themes)
Plan 3's Settings → **Look** tab becomes **Themes** (Plan 3 Task 20 is extended, not forked).
1. Settings → Themes shows a list. The first row is **Standard** ("Built in — the style RustyPresenter started with"); then any built-in themes from the registry (§4.2; none in FEAT-1 itself, FEAT-7 adds "Livestream lower third"); then saved themes in name order. Each row shows "Used by N items" when N > 0. The row that is the church default shows a filled star and "Church default — used whenever nothing else is picked"; the others show an empty star button (tooltip "Make this the church default"). Row buttons: **Edit**, **Duplicate**, **Delete** (saved themes only). A built-in row offers Delete as **Reset** once edited (it removes the override); before that it has no Delete, and a stale request is refused with "Built-in themes can't be deleted." Under the list: **New theme** and the **Logo** section (moved here from Look; it saves at once, see step 7).
2. **New theme** opens the Theme editor pre-filled with a copy of the current church default, named "New theme". **Duplicate** does the same from that row, named "<name> copy" (suffixed " 2", " 3"… when taken).
3. The **Theme editor** (wide dialog, stacked over Settings):
   - Left column: **Name** (Standard's is fixed); **Font** (bundled fonts first, marked "works on every computer", then the 20 web fonts, then "More fonts on this computer…" which opens a searchable list of installed families); **Weight** Regular / Semibold / Bold ("for songs and announcements"); **Lyrics size** 40–150 "(biggest; long lines shrink to fit)"; **Scripture size** 36–120; **Text color**; **Align** Left / Center / Right; **Position** Top / Middle / Lower third; **ALL CAPS**; **Shadow**; **Outline** Off / Thin / Thick + colour; **Text band** Off / On + colour + "See-through" 0–90 %; **Background** (the background picker, §3.4; for a saved theme its first choice is "Standard background").
   - Right column: a 16:9 preview with three tabs — **Lyrics**, **Scripture**, **Announcement** — using fixed sample text ("Amazing grace how sweet the sound / That saved a wretch like me", John 3:16 KJV, an announcement with a bold heading). Re-rendered 150 ms after each change, over the chosen background, exactly as the room draws it.
   - When the chosen font is not installed: "“<font>” isn't installed on this computer, so slides use Noto Sans." (Plan 3 decision 8, moved here.)
   - Footer: **Cancel** and **Save theme**.
4. **Save theme** → `theme/save` (Standard: `theme/save-standard`) → closes the editor, toast "Theme saved", the list refreshes. `theme/save` returns `{id, screen_views}`; when `screen_views` is not empty (FEAT-7), the toast adds "Used by screen view(s): <names>".
5. **Star** on a row asks "Make “<name>” the church default? Everything without its own theme will use it. You can switch back any time." → `theme/make-default {id}`, toast "“<name>” is now the church default". Nothing is copied: the star is a pointer, so editing the starred theme later changes the default, and starring Standard again in January brings the normal look back.
6. **Delete** asks "Delete the theme “<name>”? N items and M services use it; they'll use the church default instead." (counts line left out when both are 0; when it is the church default, the question adds "It's the church default, so Standard becomes the church default."). → `theme/delete`, toast "Theme deleted".
7. **Logo**: **Choose logo…** / **Remove logo** call `settings/logo` immediately (like `settings/translation`), toast "Logo saved" / "Logo removed".

### 3.2 Picking a theme for a service
1. The Service column header gets a small chip under the service name: "Theme: Church default" (or the theme's name). Clicking it opens a menu: **Church default (<name>)**, each built-in and saved theme, **Manage themes…** (opens Settings → Themes).
2. Picking runs `service/theme`. Thumbnails re-render. The room is not changed (§5.3). If the theme has its own background and N > 0 items of this service have their own background, the operator asks "N items have their own background. Use the theme's background for them too?" **Use theme background** (runs `service/clear-item-backgrounds`) / **Keep theirs**.
3. If the live item belongs to this service and its resolved theme changed, FEAT-4's change banner appears in the Live column (reason `LookOnScreen`) with **Update screen** (`Command::Refresh`). FEAT-1 has no line of its own for this (§5.3).
4. Item rows with their own background show " · own background" in their subtitle, so a half-themed service explains itself.
5. The Services dialog row shows the theme name after "N item(s)" when it isn't the church default. **Copy** of a service keeps its theme.

### 3.3 Picking a theme for one item
Item edit (Plan 3 Task 18) becomes scrollable with sections Words, Theme, Automation, Stage and Bible (§7.2). For song, scripture and announcement items its **Theme** section holds a **Theme** combo above the background picker: "Same as service (<name>)" (default) and each built-in and saved theme. The background picker's first choice is renamed "From theme". The item row's subtitle adds " · <theme name>" when the item has its own theme. Deck and media items show no theme combo (they don't draw text).

### 3.4 Backgrounds
The background picker (Plan 3 Task 18 `BackgroundPicker`, used in Item edit, the Theme editor and the Backgrounds manager) becomes sectioned:
0. **Current** (only when the current value matches no other choice, e.g. a background removed from the library): "<thumbnail> Current (not in your backgrounds)", selected, so Save keeps it.
1. **From theme** (Item edit only) or **Standard background** (saved-theme editor only).
2. **Colors & gradients**: the six existing swatches, six built-in gradients (§4.3), **Custom color…**, **Custom gradient…** (two colour pickers + angle 0/45/90/135/180).
3. **Backgrounds**: the library's pictures and videos (thumbnail, name as tooltip, "▶" on videos). An entry still converting shows "Converting 40 %" and can't be picked; a failed one shows "Couldn't convert" with Remove.
4. **From your media**: ready images and videos from the media library not already in Backgrounds (so nothing that worked before disappears).
5. **Add picture or video…** (native file dialog, several files allowed) imports with `ImportPurpose::Background`, selects the first ready one, toast "Added to backgrounds". **Manage…** opens the Backgrounds manager.
6. When the selected choice is a picture, video or gradient, a **Darken** slider (0–80 %, default 0) appears under the grid with help text "Makes text easier to read".

The **Backgrounds manager**: a grid of library entries with **Rename** and **Remove**, "Add picture or video…", and a search box above 24 entries. Remove asks "Remove “<name>” from backgrounds? Items that use it keep it." and never changes any item or theme (backgrounds are values, §4.3). Dropping picture/video files on the manager imports them as backgrounds; dropping them on the operator window still adds them to the service (Plan 3 Task 15).

### 3.5 Rich text in announcements
In Item edit for an announcement, the body field becomes the **rich text editor**:
1. A toolbar: **B**, *I*, U̲, a colour button (the six text swatches, gold `#f5b94a`, red `#ff4f55`, "Theme color"), a size menu (Small 75 % / Normal / Large 125 % / Huge 150 %), **Clear formatting**. Tooltips: "Bold (Ctrl+B)", "Italic (Ctrl+I)", "Underline (Ctrl+U)", "Text color", "Text size", "Remove bold, color and size from the selection". When the resolved theme is already Bold, **B** is disabled with the tooltip "Text is already bold in this theme".
2. Select text and click a button → the selection gets that style. With nothing selected, the button sets the style for what is typed next at the cursor (the button shows as pressed until the cursor moves).
3. The text box shows plain text (Slint has no editable rich text) inside a scrolling area of fixed height (8 lines), so long bodies never push the dialog off screen. Under it, a **Formatting** list shows one chip per formatted range, e.g. "Bold: ‘Sunday 10am’", "Gold, Large: ‘Pancake breakfast’" (text cut at 24 characters with "…"); clicking a chip selects that range in the box; its × clears that range's formatting. Under that, a **preview strip** shows every slide as the room will draw it (rendered 250 ms after the last change). The preview is the source of truth for how it looks.
4. A blank line still starts a new slide; help text kept: "Leave an empty line between parts to make more than one slide."
5. Ctrl+B / Ctrl+I / Ctrl+U work while the editor has focus. Plan 3's key routing changes for this (§7.1 `keys.rs`): with a dialog open, Ctrl+B is passed through to the focused field instead of being swallowed.
6. Pasted text arrives plain and takes the style of the text before the cursor.

### 3.6 Overlays (operator)
1. The Live column gets an **Overlays** strip (its place in the column is fixed by the layout budget, §7.2): a single row, fixed height 36 px, scrolling horizontally when it overflows (no wrapping, so the Live column's height never changes on a 1366×768 laptop). One chip per saved overlay in library order (name + small kind icon), lit (accent) when on screen; at the right end **F6** (Clear overlays) and **+** (opens the Overlays manager). With no saved overlays the strip collapses to one small "+ Overlays" button (tooltip "A logo in the corner, a clock or a short sign").
2. Clicking a chip toggles it: `Overlay { id, on: !lit }`. The overlay fades in or out (400 ms). It stays up while slides, items and media change. **Logo (F5)** keeps it (pictures, clocks and text signs all survive the logo; FEAT-2 timers don't, §5.2). **Clear All (F1)** and **Clear overlays (F6)** remove all overlays. Up to 8 saved overlays can be on at once. A chip turned on after a clear the operator hadn't seen yet (from the phone or another operator) is refused with "The screen was cleared — try again." (§5.2).
3. The Overlays manager (dialog): list with Edit / Duplicate / Delete and drag-to-reorder (order = drawing order, last on top), **New overlay** with three kinds: **Text** — a sign that stays up ("Wi-Fi: GraceGuest", "Welcome!"), **Picture** (a logo bug), **Clock**. The Text kind's help line: "For a sign that stays up. For a notice like a parent's name, use Message (once available)." (the last part only once FEAT-2 ships).
4. The **Overlay editor**: Name; for Text the rich text editor (one line or a few); for Picture **Use the church logo** (default; follows the logo when it changes) or **Choose picture…**, and opacity; for Clock "Show seconds" and "24-hour clock". Then **Where** — a 3 × 3 grid of positions (default bottom-right for pictures and clocks, bottom-center for text); **Size** slider (5–100 % of screen width); for Text and Clock: font (default "Same as church default"), text size 20–120, colour, shadow, **Box** Off / Behind the text / Full-width bar + colour + "See-through". Preview: the overlay over a sample lyric slide on the church default background.
5. Editing an overlay that is on screen does not change the room. Its chip shows a small dot and tooltip "Changed — turn it off and on to update." Deleting an overlay that is on screen leaves it up; its chip stays (struck-through name) until turned off or cleared.
6. If an overlay can't be shown (its picture is missing), it is taken off, its chip goes dark, and the Live column notice says "Couldn't show the overlay “<name>”: the picture could not be loaded." Slides keep working. The notice is short-lived (`live.notice`): the next visual change clears it.

### 3.7 Phone remote
The phone page (`web/remote.*`) gets an **Overlays** row under the clear buttons: the saved-overlay chips (lit when on) and "Clear overlays". It never edits themes, backgrounds or overlays. FEAT-1 adds exactly `Overlay { id, on }` and `ClearOverlays` to the phone allowlist, whose only table is FEAT-2 §5.11 (§5.6). Turning an overlay on after a clear the phone hadn't seen shows "The screen was cleared — try again."

### 3.8 Room and stage effect
- Room: text slides use the resolved theme captured when the item went live; overlays draw above everything else visual (§6.1).
- Operator live preview (Plan 3 `src/ui/preview.rs`): shows overlays exactly as the room does. Slide thumbnails never show overlays.
- Stage: unchanged (plain current/next text, no overlays, no theme).

## 4. Data model

All new fields carry serde defaults; old `data.json` and `live-state.json` load unchanged. New types live in new modules re-exported from `model.rs`: `src/theme.rs`, `src/richtext.rs`, `src/backgrounds.rs`, `src/overlay.rs`.

### 4.1 Text style (`model::Theme`, extended)
The struct keeps its name and fields (Standard stays at `Settings.theme`). It stays `Eq + Hash` (part of `RenderKey`), so every new field is an integer, bool, string or fieldless enum.
```rust
#[serde(default)]
pub struct Theme {
    pub font: String, pub size: u32, pub color: String, pub shadow: bool,
    pub position: String,          // "center" | "lower" | "top" (new value)
    pub uppercase: bool, pub scripture_size: u32,
    pub weight: u16,               // new: 400 | 600 | 700, default 600. Lyrics and announcements only;
                                   //      scripture always draws at 500, as layout.rs does today
    pub align: TextAlign,          // new: Left | Center | Right, default Center (lyrics + announcements; scripture stays Left)
    pub outline: u8,               // new: 0 off, 3 thin, 6 thick (logical px), default 0
    pub outline_color: String,     // new: default "#000000"
    pub band: bool,                // new: default false
    pub band_color: String,        // new: default "#000000"
    pub band_opacity: u8,          // new: 0–100, default 55
}
```
`Theme::default()` sets the new fields as above, so existing data renders pixel-identically (goldens, §9.1).

### 4.2 Saved themes, the church default, built-ins
```rust
// src/theme.rs
pub type ThemeId = Uuid;
pub const STANDARD_ID: ThemeId = uuid!("00000000-0000-4000-8000-000000000101"); // Settings.theme + Settings.background
pub struct SavedTheme {
    pub id: ThemeId,
    pub name: String,
    pub style: Theme,
    #[serde(default)] pub background: Option<Background>, // None = Standard background
}
pub fn builtin() -> Vec<SavedTheme>;            // read-only registry, fixed ids; FEAT-1 ships it empty
pub fn find(data: &Data, id: ThemeId) -> Option<SavedTheme>; // Standard, then Data.themes, then builtin()
pub fn church_default(data: &Data) -> SavedTheme;           // find(default_theme) or Standard
pub struct Resolved { pub style: Theme, pub background: Background, pub from: ThemeSource }
pub enum ThemeSource { Item(ThemeId), Service(ThemeId), Church(ThemeId) }
pub fn resolve(data: &Data, service: Option<&Service>, item: &ServiceItem) -> Resolved;
pub fn service_of(data: &Data, item_id: Uuid) -> Option<(&Service, &ServiceItem)>; // searches all services
```
- `Data.themes: Vec<SavedTheme>` and `Data.default_theme: Option<ThemeId>` (both `#[serde(default)]`). They live in `Data`, not `Settings`, so the Settings dialog's whole-struct save can never overwrite them. `None` or a dangling id = Standard.
- **Standard** is the existing `Settings.theme` + `Settings.background`, exposed as a `SavedTheme` with `STANDARD_ID` and name "Standard". It can be edited, never deleted. `LiveView.theme` (used by the stage and old pages) keeps meaning "the church default's style".
- **Built-ins** (`theme::builtin()`) are code-defined with fixed ids and read-only. They are never seeded into `Data.themes` (there is no `theme::seeded`). Saving one stores an override in `Data.themes` with the same id; `find` prefers the override; Delete on a built-in is offered as **Reset**, which deletes the override. Deleting a built-in id without an override is refused: "Built-in themes can't be deleted." FEAT-7's `STREAM_LOWER_THIRD_THEME` lives here, in `theme::builtin()`.
- `Service.theme: Option<ThemeId>` and `ServiceItem.theme: Option<ThemeId>`, both `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- **Resolution.** Theme = the first of item theme, service theme that `find` resolves; otherwise `church_default`. Background = `item.background` if set; else the chosen theme's `background` if set; else Standard's (`Settings.background`). Only the chosen theme counts. A dangling id is skipped (§5.7).

### 4.3 Backgrounds
```rust
pub enum Background {          // model.rs, tag = "type"
    Color { color: String },
    Image { asset: AssetId, #[serde(default)] dim: u8 },          // dim: 0–80 %
    Video { asset: AssetId, #[serde(default)] dim: u8 },
    Gradient { from: String, to: String, #[serde(default)] angle: u16, #[serde(default)] dim: u8 }, // new
}
// src/backgrounds.rs
pub struct BackgroundEntry {
    pub id: Uuid, pub name: String, pub background: Background,
    #[serde(default = "ready")] pub status: MediaStatus,   // videos may be Converting / Failed
    #[serde(default)] pub thumb: Option<AssetId>,   // video: frame at 1 s (Plan 4 mpv probe); else None
    pub added: chrono::DateTime<Utc>,               // RFC 3339; migration (§4.7) stamps the migration time
}
pub fn builtin_backgrounds() -> Vec<(&'static str, Background)>; // 6 colours (Plan 3 swatches) + 6 gradients
```
- `Data.backgrounds: Vec<BackgroundEntry>` (`#[serde(default)]`).
- Time types follow the cross-spec rule: `chrono` is the only time crate (never `jiff`); persisted instants are `chrono::DateTime<Utc>` serialized as RFC 3339, persisted dates `chrono::NaiveDate` (YYYY-MM-DD), times of day FEAT-2's `TimeOfDay` (or `chrono::NaiveTime` at runtime). The clock's `BoundCtx.now` (§4.5) is runtime-only and may be `DateTime<Local>`.
- Built-in gradients: "Midnight" `#0b1a33→#000000` 180°, "Deep purple" `#2b1530→#0d0f13` 135°, "Forest" `#1d2b1f→#050805` 180°, "Ember" `#3a2412→#0d0806` 135°, "Ocean" `#0e3a4f→#06141c` 180°, "Slate" `#2a303b→#0d0f13` 180°.
- **Backgrounds are values.** Items, themes and settings store a `Background` value, never an entry id, so removing or renaming an entry never changes what anything shows; `referenced_assets` keeps the asset alive while anything uses it.
- **Video conversion.** A background video that `media::plan_video` says needs converting is added with `status: Converting { percent: 0 }` and converted by `spawn_conversion`, generalised to a `ConvertTarget::{Media(Uuid), Background(Uuid)}` so progress and the final asset swap land on the right record. On success the entry's `background.asset` becomes the converted asset (the original is not kept: a background never needs it) and `status = Ready`. Only Ready entries can be picked, so no item ever stores an unconverted asset.
- `dim` is drawn as a black layer of that opacity over the picture/video/gradient (§6.3). It is part of the value.

### 4.4 Rich text
```rust
// src/richtext.rs
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RunStyle {
    #[serde(default, skip_serializing_if = "is_false")] pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")] pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")] pub underline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub color: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")] pub size: u16,   // percent; 0 = 100
}
pub struct StyleSpan { pub start: u32, pub end: u32, #[serde(flatten)] pub style: RunStyle } // byte offsets, end exclusive
```
- Text stays a plain `String` everywhere (search, stage, phone and the Tauri app keep working). Formatting is a sorted list of non-overlapping, non-empty spans. Unstyled text has no span.
- `ItemKind::Text { title, body, #[serde(default, skip_serializing_if = "Vec::is_empty")] styles: Vec<StyleSpan> }`. On save the core normalises the body to `\n` line ends and the spans to that text.
- `SlideContent::Text` and `live::SlideLayer::Text` gain `styles`, relative to that slide's text. `slides::text_slides` cuts blocks (blank line = new slide, trimmed as today) and slices spans with `richtext::slice(spans, block_range)`.
- Pure functions: `normalize(text, spans)` (snap to char boundaries, clamp, drop empty, sort, merge equal neighbours, split overlaps last-wins), `apply(spans, range, f)`, `clear(spans, range)`, `adjust(spans, edit_start, removed, inserted, inherit)`, `slice(spans, range)`, `diff_edit(old, new) -> (start, removed, inserted)`, `uppercase(text, spans)` (span by span; "ß" → "SS" changes lengths), `ranges(text, spans) -> Vec<(Range, RunStyle, String)>` (for the Formatting chips).

### 4.5 The overlay layer (generic slots)
```rust
// src/overlay.rs
pub type SlotKey = String;                 // "saved:<uuid>" | "timer:<uuid>" (FEAT-2) | "message" (FEAT-2)
pub fn saved_key(id: Uuid) -> SlotKey;     // FEAT-2 adds timer_key(id) and MESSAGE_KEY
pub struct OverlaySlot {
    pub key: SlotKey,
    pub z: u32,                            // saved: library index; FEAT-2 timers 1_000_000 + index; message u32::MAX
    pub inst: u64,                         // from Live.next_inst at show time; failure reports name it (§5.3)
    pub name: String,                      // for chips, notices and the phone
    pub source: OverlaySource,
    pub place: Placement,
}
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverlaySource {
    Text  { text: String, #[serde(default)] styles: Vec<StyleSpan>, look: OverlayLook },
    Image { source: ImageSource, #[serde(default = "full")] opacity: u8 },   // 0–100
    Clock { look: OverlayLook, #[serde(default)] seconds: bool, #[serde(default)] h24: bool },
    // FEAT-2 adds: Timer { timer_id, caption, large, look }, Message { shown, template_id, text, look }
    #[serde(other)] Unknown,               // a newer source kind: never drawn, removed by every clear
}
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource { ChurchLogo, Asset { asset: AssetId } }
pub struct OverlayLook {           // #[serde(default)]
    pub font: String,              // "" = church default font
    pub size: u32,                 // px at 1080p, default 48
    pub color: String,             // default "#ffffff"
    pub weight: u16,               // default 600
    pub shadow: bool,              // default true
    pub box_kind: BoxKind,         // None | Behind | Bar, default None
    pub box_color: String,         // default "#000000"
    pub box_opacity: u8,           // default 60
}
pub struct Placement { pub anchor: Anchor, pub width: u8, pub margin: u8 } // width % of screen (5–100); margin logical px /10 (default 4)
pub enum Anchor { TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom, BottomRight }
pub struct Overlay { pub id: Uuid, pub name: String, pub source: OverlaySource, pub place: Placement } // saved library entry
pub const OVERLAY_FADE_MS: u32 = 400;     // show, hide and in-place replacement
pub const MAX_SAVED_ON: usize = 8;
```
- `Data.overlays: Vec<Overlay>` (`#[serde(default)]`); its order is the drawing order. `overlay/save` accepts only `Text`, `Image`, `Clock`.
- **Live frame.** `live::Frame` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub overlays: Vec<OverlaySlot>`, kept sorted by `(z, key)`, at most one slot per key. The frame holds a **copy** of the content as it was when shown (like `LiveItem` snapshots). For `ImageSource::ChurchLogo` the copy is resolved when shown: the slot stores `Asset { asset: Settings.logo }` (no logo → "Choose a church logo first (Settings → Themes)."), so the logo bug picks up a new logo the next time it is turned on.
- **One placement type.** FEAT-2's `OverlayPlace` and `MessagePlace` stay as the small choice lists in FEAT-2's editors; FEAT-2 maps them to `Placement` when it builds a slot: `TopLeft/TopRight/BottomLeft/BottomRight` → the same `Anchor`, width 25, margin 4; `CenterLarge` → `Center`, width 80; message `Bottom`/`Top` → `Bottom`/`Top`, width 100, `BoxKind::Bar`.
- **Final names** (all specs use them): `live::Frame.overlays: Vec<OverlaySlot>`; `OverlaySlot { key: SlotKey, z, inst, name, source: OverlaySource, place: Placement }`; `OverlaySource { Text, Image, Clock, Timer{..}, Message{..}, #[serde(other)] Unknown }` is the one content enum, owned by FEAT-1, with FEAT-2 adding the `Timer` and `Message` arms; a `SlotKey` is `"saved:<uuid>"`, `"timer:<uuid>"` or `"message"`, built by `saved_key(id)` here and FEAT-2's `timer_key(id)` and `MESSAGE_KEY`. The names `OverlayOn`, `OverlayContent`, `MESSAGE_SLOT` and `OverlayOn.id` are retired. FEAT-7 classifies slots with `OverlayClass { Saved, Timer, Message }` and `pub fn class_of(slot: &OverlaySlot) -> OverlayClass`, matching on `slot.source` (`Unknown` counts as `Saved`).
- **Logo policy per source** (fixed in code, `OverlaySource::survives_logo(&self) -> bool`): `Text` yes, `Image` yes, `Clock` yes, `Message` yes, `Timer` **no**, `Unknown` no.
- **The 8 cap** counts only `saved:` slots. FEAT-2 keeps its own limits (at most 4 timers, one message slot).
- **Bound content.** `Clock` and FEAT-2's `Timer` are *bound*: text computed when drawn by `overlay::bound_text(&OverlaySource, &BoundCtx) -> Option<String>`. FEAT-1's `BoundCtx { now: chrono::DateTime<Local> }`; FEAT-2 adds `timers: Arc<TimersView>`. Bound text never touches `Live.version` (§5.5).

### 4.6 Live engine types
- `LiveItem` gains `pub theme: Theme` (resolved style) next to its resolved `background`; `DataPlan::build` fills both from `theme::resolve` with the item's own service (`theme::service_of`). FEAT-4's reconcile reads `LiveItem.theme` (and `background`) to detect a theme difference on the live item (§5.3).
- `SlideLayer::Text` gains `#[serde(default)] theme: Option<Theme>` and `styles`. `go()` sets `theme: Some(item.theme.clone())`. `None` (old `live-state.json` only) means "use `LiveView.theme`".
- `Command` gains `Overlay { id: Uuid, on: bool, #[serde(default)] seen_clear: Option<u64> }` and `ClearOverlays`. `seen_clear` is required when `on` is true (§5.2) and ignored when false.
- `Live` gains `pub clear_epoch: u64` (`#[serde(default)]`, saved in `live-state.json` so a restart never lowers it) and `LiveView` exposes it, so every operator and the phone know the newest clear they have seen.
- Private mutators, no version bump inside (the caller commits; FEAT-2's `mutate`/`apply_batch` calls them):
  `fn show_overlay(&mut self, slot: OverlaySlot) -> bool` — insert or replace by key; returns false when an equal copy (ignoring `inst`) is already there.
  `fn hide_overlay(&mut self, key: &str) -> bool` — remove; false when absent.
  `fn retain_overlays(&mut self, f: impl Fn(&OverlaySlot) -> bool) -> bool` — used by Logo and the clears.
- `live::Plan` gains `fn saved_overlay(&self, id: Uuid) -> Option<(u32, Overlay)>` (library index, copy) and `fn logo(&self) -> Option<AssetId>`; `DataPlan` reads `Data.overlays` and `Settings.logo`.
- `RoomEvent` gains `OverlayFailed { key: String, inst: u64, reason: String }`.
- `LiveView` keeps `theme` (= church default style). There is no `theme_stale` flag: a theme difference on the live item is FEAT-4's `Live.changed: LiveChange { reason: LookOnScreen }` (§5.3). `live.notice` holds only short-lived notices (FEAT-1's overlay failure, FEAT-2's) and is cleared on the next `Visual` change.

### 4.7 Migration (format 1 → 2), newer data and the load hook
`store.rs` holds the **only** format constant, `pub const SUPPORTED_FORMAT: u32`. FEAT-1 sets it to 2; FEAT-6 sets it to 3 (a format-2 build could not parse `ItemKind::Blank`). Any later spec that adds a variant to a persisted enum without `#[serde(other)]` bumps it. The **migration chain** lives in `store::load`: one step per format (1 → 2 here, 2 → 3 in FEAT-6, …), run in order, each taking its own "before-update" backup first (step 1 below). No other module keeps a format number or migrates.

`store::load` first reads only `{"format": u32}` from `data.json`:
- **Newer data** (`format > SUPPORTED_FORMAT`): the app opens **read-only**. It tries a full parse; if that fails it starts with empty data. A banner says "This data was saved by a newer RustyPresenter. Please install the latest version." Every `do_action` that edits returns that message; `store.save` refuses; live control (showing what exists) still works and `live-state.json` is still written. Only builds that include this guard are protected; the release notes say so.
- **Format < 2**: migrate once, at load, before the app starts:
  1. `store.backup_labeled(&data, "before-update")`. If it fails (e.g. "asset … is missing"), copy `data.json` to `data.before-update-<stamp>.json` instead, log the missing assets, and continue — startup is never blocked.
  2. For every distinct image/video `Background` used by `Settings.background` or any `ServiceItem.background`, add a `BackgroundEntry` (name = the matching `MediaFile.name`, else "Background 1", "Background 2", …; status Ready). Nothing else changes: no themes, no overlays, no spans; `default_theme` stays `None` (Standard).
  3. Set `format = 2` and **save immediately**, so the migration and its backup never run again. (Later steps of the chain follow the same pattern.)
- **Load hook.** After the chain, `store::load` calls exactly one hook, `Data::after_load(&mut self) -> bool` (true = something changed, save now). It runs every fix-up in a fixed order: FEAT-2's automation seed, FEAT-5's fix-ups, FEAT-7's screen-view and usage cleanup, and `Settings::normalize`. FEAT-1 itself only adds `richtext::normalize` of stored spans here. No other load hook exists; every spec adds its fix-up inside `after_load`. In read-only (newer-data) mode `after_load` still runs but its result is never saved.
- `Store::backup_labeled(data, label)`: like `backup`, directory `backup-<stamp>-<label>`, and `prune_backups` keeps the newest labelled backup per label outside the normal `KEEP_BACKUPS` count, so app updates never push out the church's own backups.

### 4.8 What an older build does with format-2 data
Builds without the guard: a `Background::Gradient` anywhere makes them refuse to start ("could not be read"); without one they load but their next save drops `themes`, `default_theme`, `backgrounds`, `overlays`, `styles` and theme ids. The "before-update" backup is the way back. Later FEAT specs bump `SUPPORTED_FORMAT` whenever they add a variant to a persisted enum without `#[serde(other)]` (FEAT-6 → 3); the guard then protects every build from FEAT-1 on.

### 4.9 Assets
`Data::referenced_assets` also walks every `SavedTheme.background`, every `BackgroundEntry` (asset and thumb) and every saved `Overlay` with `ImageSource::Asset`. Rule for any future asset cleanup (none exists today): it must keep the union of `referenced_assets()` and the assets of the live and committed frames (background, media, overlay slots), so deleting an overlay or background that is on air never deletes its file.

## 5. Core behaviour

### 5.1 Actions (`App::do_action`)
| Action | Body → result | Rules / errors |
|---|---|---|
| `theme/save` | `SavedTheme` (nil id = new) → `{id, screen_views: [..]}` (names of the screen views using the theme; always empty before FEAT-7) | Name trimmed, required: "Give the theme a name."; unique case-insensitively among saved and built-in names: "There's already a theme called “<name>”."; "Standard" and "Church default" are reserved: "That name is used by a built-in theme. Pick another."; sizes clamped like `settings` (24–200), `weight` to 400/600/700, `outline` ≤ 12, `band_opacity` ≤ 100. A built-in id stores an override. |
| `theme/save-standard` | `{style: Theme, background: Background}` → `{}` | Same clamps; writes `Settings.theme`/`Settings.background` only. |
| `theme/make-default` | `{id}` → `{}` | Sets `Data.default_theme` (`STANDARD_ID` → `None`). Unknown: "That theme is gone." |
| `theme/delete` | `{id}` → `{items, services, was_default}` | Clears every `ServiceItem.theme`/`Service.theme` equal to `id`, and `default_theme` when it pointed here, in the same edit. Built-in without override: "Built-in themes can't be deleted."; built-in with override = Reset. |
| `theme/usage` | `{id}` → `{items, services}` | For the delete question and "Used by N items". |
| `service/theme` | `{service_id, theme: Option<ThemeId>}` → `{own_background: N}` | Unknown theme: "That theme is gone." `N` = items of that service with `background: Some(_)`, reported only when the theme has a background (else 0). |
| `service/clear-item-backgrounds` | `{service_id}` → `{cleared}` | Sets `background = None` on that service's song/scripture/announcement items. |
| `service/update` | unchanged shape | **Merges by key** (the rule below). Covers every new `Service` and `ServiceItem` field from every FEAT spec: `theme`, `styles`, `starts_at`, `advance`, `stage_layout`, `slide_notes`, `arrangement`, `second`, `layout`, `breaks`, `pco_item_id`. So the Tauri web operator, which doesn't know them, can't wipe them. `styles` are normalised against `body`. |
| `background/add` | `{name, background}` → `{id}` | For colours/gradients saved from Custom…; name required: "Give the background a name." |
| `background/rename` / `background/remove` | `{id, name}` / `{id}` → `{}` | "That background is gone." |
| `overlay/save` | `Overlay` (nil id = new) → `{id}` | "Give the overlay a name."; `width` 5–100; text needs text: "Type the overlay's text."; image asset must exist; source must be Text/Image/Clock. |
| `overlay/delete` / `overlay/move` | `{id}` / `{id, to}` → `{}` | Delete leaves a live copy on screen. |
| `settings/logo` | `{asset: Option<AssetId>}` → `{}` | Asset must exist: "That picture is gone."; writes `Settings.logo` only; toast "Logo saved" / "Logo removed". |
| `settings` (settings/save) | partial `Settings` → `{}` | **Merges by key** (the rule below). The Settings dialog sends only the fields its tabs edit; this replaces Plan 3's "Save keeps live fields" copy list. |

**Merge rule** (`settings/save` and `service/update`): a missing key keeps the stored value; an explicit `null` clears an optional field; a present value replaces it. Bodies are read into structs whose fields are `Option<T>` (`Option<Option<T>>` for optional fields). Fields edited elsewhere are therefore always safe from a Settings save: `theme`, `background`, `logo` (`theme/save-standard`, `settings/logo`), `translation`, `displays`, `bible`, `stage`, `print`, `stream` and `planning_center`.

`App::import_path` gains `ImportPurpose::Background`: images and videos add a `BackgroundEntry` (named after the file without extension) instead of a `MediaFile`; videos run `plan_video` and, if needed, `spawn_conversion(ConvertTarget::Background(id))`; other kinds fail with "Backgrounds can be pictures or videos." Video thumbs come from the mpv probe once Plan 4 lands; until then black with ▶.

Every action that changes data broadcasts `Update::Data`. Every action goes through `App::edit`, after which FEAT-4's reconcile runs; so any action that can change theme resolution (`theme/save`, `theme/save-standard`, `theme/make-default`, `theme/delete`, `service/theme`, `service/clear-item-backgrounds`, `service/update`, `settings`) surfaces a theme difference on the live item as FEAT-4's change banner at once (§5.3). FEAT-1 adds no rebroadcast of its own.

### 5.2 Live commands and the combined clear matrix
`Live::apply` (`src/live.rs`):
- `Overlay { id, on: true, seen_clear }`: refused with "The screen was cleared — try again." when `seen_clear` is missing or `< clear_epoch` (the clear-epoch rule below). Then `plan.saved_overlay(id)` or "That overlay is gone."; resolve `ChurchLogo`; build the slot (key `saved:<id>`, z = library index, new `inst`). If the 8 saved slots are full and this key isn't one of them → "Up to 8 overlays can be on at once. Turn one off first." Then `show_overlay`; bump only if it returned true (idempotent: a phone and the laptop toggling on together don't flicker; a FEAT-2 macro turning on an edited overlay shows the latest version).
- `Overlay { id, on: false }`: `hide_overlay("saved:<id>")`; bump only if true.
- `ClearOverlays`: `retain_overlays(|_| false)`; bump only if something was removed.
- `ClearAll`: `Frame::default()` empties overlays.
- `Logo`: sets the visual layers as today (it must not reset the frame) and `retain_overlays(|s| s.source.survives_logo())`.
- `ClearSlide`, `ClearMedia`, `ClearAudio`, `Show`, `Next`, `Prev`, pause commands: never touch overlays.
- Overlay commands carry **no `base`**, from the operator and the phone. Command ids and duplicate detection (v1 §5.4) apply unchanged.

**Clear epoch: a clear always wins over a delayed show without a base.** `Live.clear_epoch: u64` is bumped by every intentional clear, whether or not it removed anything: F1–F6 (`ClearAll`, `ClearSlide`, `ClearMedia`, `ClearAudio`, `Logo`, `ClearOverlays`), the phone's clears and Logo, and macro Clear/Logo steps (FEAT-2). Every command that shows something without a `base` carries `seen_clear` (the `clear_epoch` from the sender's latest `LiveView`): `Overlay { on: true }` here, and FEAT-2's `ShowMessage`, timer show and the overlay/message/timer show steps of macros (an at-zero macro captures the epoch when its timer starts). Such a command is refused when `seen_clear < clear_epoch`; the phone and operator show "The screen was cleared — try again.", and a macro skips only those show steps and says so in its toast. Hide and off commands (`Overlay { on: false }`, `ClearOverlays`, FEAT-2's hides) stay baseless and are always allowed. Relative commands (`Next`, `Prev`, FEAT-2 relative macros) keep needing a fresh `base` as before.

**Practice (FEAT-6 §5.5 routing).** `Overlay { on: true }` goes to the practice engine while practice is on. `ClearOverlays` (F6), `Overlay { on: false }` and every other clear or off command reach **both** engines (room and practice), from the operator, the phone and macro clear steps, so a clear always reaches the room.

**Combined clear matrix (v1 §6.2) — one row per overlay source; FEAT-1 owns this table**

| Overlay source | Clear All F1 | Clear Slide F2 | Clear Media F3 | Clear Audio F4 | Logo F5 | Clear overlays F6 |
|---|---|---|---|---|---|---|
| Text sign (FEAT-1) | Removed | Stays | Stays | Stays | Stays | Removed |
| Picture / logo bug (FEAT-1) | Removed | Stays | Stays | Stays | Stays | Removed |
| Clock (FEAT-1) | Removed | Stays | Stays | Stays | Stays | Removed |
| Message (FEAT-2) | Removed | Stays | Stays | Stays | Stays | Removed |
| Timer (FEAT-2) | Removed | Stays | Stays | Stays | **Removed** | Removed |
| Every other layer | as v1 §6.2 | | | | | Stays |

Why timers go on Logo: Logo means "clean screen, just our logo". A pre-service countdown left over the logo once the service has started is wrong, and FEAT-6's "Show the logo, then practice" relies on Logo hiding it. Pictures, clocks, signs and messages are things the operator chose to keep up regardless (a nursery message must not vanish because someone pressed Logo). The incoming-item policy (v1 §6.3) gets one rule: **an incoming slide of any kind leaves overlays unchanged.**

### 5.3 Theme snapshot, preparation failures and live safety
- The resolved style and background are captured in `LiveItem` at Take Live / `Show` (v1 §7.2). `Next`/`Prev` within the item use the snapshot, so editing a theme, picking a service theme, starring a default or editing Standard **never changes the room by itself**. Today's v0.2 behaviour (a Look save restyles the room instantly) is deliberately changed to follow the live-safety rule.
- The room's slide-layer key uses the slide layer's own `theme` (falling back to `LiveView.theme` only when `None`).
- **Edited live item: one mechanism, FEAT-4's reconcile.** FEAT-1 has no stale flag or line of its own. Reconcile (FEAT-4) runs after `App::edit` and on `Update::PlansReady`, for every item kind; it finds the live item with `theme::service_of` (searching **all** services) and compares `theme::resolve`'s `(style, background)` with the snapshot in `LiveItem.theme` / `LiveItem.background`. A difference counts as `LiveChange { reason: LookOnScreen }` in `Live.changed`; FEAT-4's change banner shows it with **Update screen** (`Command::Refresh`), which rebuilds the item with the new theme and maps the position in the core (songs by `SectionRef`, scripture by FEAT-5's `slides::index_for_source`, other items by index). The snapshot never changes the room unasked. Clicking a slide of the live item (`Show`) also rebuilds it with the new theme.
- A missing font is not a failure: rendering falls back to Noto Sans (native spec §4).
- **Overlay preparation never blocks slides.** In the room's prepare step, each overlay slot is prepared on its own. A slot that fails (picture unreadable, asset missing) is left out of the prepared set; the room commits all other layers as usual and sends `RoomEvent::OverlayFailed { key, inst, reason }`. `Live::report` handles it: if a slot with that key **and inst** is still in the frame, `hide_overlay(key)`, bump, and set the short-lived notice (`live.notice`, cleared on the next `Visual` change) "Couldn't show the overlay “<name>”: <reason>". Stale reports (slot already replaced or gone) are ignored. Because the slot is removed, the next plan no longer contains it, so nothing retries in a loop. This covers a fresh toggle, a crash restore whose `live-state.json` holds a missing asset, a room window reopened after a monitor unplug, and an overlay deleted while its copy is live.
- Background or slide preparation failure keeps the existing path (`Failed` → revert to the committed snapshot). Overlay slots are never part of that revert decision.

### 5.4 Navigation
No change to the cue list or the transition table (v1 §7.2). Overlays are not cues. "Up next" on the stage and phone is unchanged.

### 5.5 Bound overlays and versions
The clock changes every second (and FEAT-2 timers too). Redrawing must not bump `Live.version`, or every phone `Next` (which carries `base`) would go stale once a second.
- The frame holds only *which* bound overlay is on and its look. The room glue and the operator preview run a 1 Hz `slint::Timer` while a `Clock` slot is on, compute `bound_text` locally and re-render only that slot's picture (cached by text). The swap has no fade and no commit. Prepare keys exclude bound text.
- FEAT-2 timer text arrives through FEAT-2's `Update::Timers` (its core tick); outputs keep the newest one in `BoundCtx.timers` and redraw the same way. FEAT-1 adds no tick update of its own.

### 5.6 Phone remote
- The phone allowlist has one table, FEAT-2 §5.11; FEAT-1's rows there are `Overlay { id, on }` (on: `seen_clear`; off: none) and `ClearOverlays` (none). `server.rs` (`remote_command`) implements that table: a fresh `base` is required for `Show`, `Next`, `Prev`, `GoToCue`, `TakeLive` and FEAT-4's `Jump`, not only `Show`; the phone sends `seen_clear` from its latest view with `Overlay { on: true }`.
- `remote_view` adds `overlays: [{id, name, on, kind}]` (saved overlays in library order, plus on-screen `saved:` slots whose overlay was deleted, named "(deleted) <name>"). Timer and message slots are not listed here (FEAT-2 has its own remote rows).
- Nothing else about themes, backgrounds or overlays is reachable from the LAN.

### 5.7 Edge cases
- **Theme deleted while an item using it is live**: the snapshot is unaffected; references are cleared; FEAT-4's change banner (`LookOnScreen`) appears if the resolved theme now differs.
- **Clear on the phone while the laptop turns an overlay on** (or the reverse): an overlay-on sent before its sender saw the clear carries an older `seen_clear` and is refused with "The screen was cleared — try again."; the clear stands.
- **Starred theme deleted**: `default_theme` is cleared in the same edit → Standard is the church default; the delete question said so.
- **Dangling item/service theme id** (restored backup): skipped → next level. Item edit shows "Same as service" and Save drops the dangling id.
- **Names differing only in case**, or "Standard"/"Church default": rejected at save.
- **Spans beyond the text** (hand-edited file): `normalize` clamps or drops them on load and save. The Tauri web operator no longer drops `styles`/`theme` thanks to the `service/update` merge; if it edits `body`, the core re-normalises the kept spans against the new body (spans past the end are dropped, others stay by offset — acceptable while both apps exist).
- **ALL CAPS with formatting**: `richtext::uppercase`. Scripture never uppercases.
- **Huge run size with auto-fit**: run sizes are relative to the fitted body size, so fitting shrinks everything together, down to 26 px base.
- **Bold run in a Bold theme**: drawn at 700, same as the rest (the B button is disabled there, §3.5). A run made bold under a Semibold theme stays bold if the theme later becomes Bold; it just no longer stands out.
- **Overlay over a full-screen deck slide**: drawn on top (a logo bug over PowerPoint).
- **Room closed**: overlay commands still change live state; when the room opens it prepares them (failures handled as §5.3).
- **Crash recovery** (native spec §8.3): `live-state.json` holds the frame, so overlays and the snapshotted theme come back. Unknown future sources load as `Unknown` and are dropped by the first clear.
- **4:3 or very small screens**: placement is relative to width and anchor; bars always span the full width.
- **Background video still converting** when picked from an old dialog: not possible (only Ready entries are offered); if the conversion fails, the entry shows "Couldn't convert".

## 6. Rendering (`src/render`)

### 6.1 Layer order
Room, operator preview and FEAT-7 compose, bottom → top: background, media, slide, logo, **overlays**, fade-to-black. Overlays sit above the logo so a clock or "Welcome" sign can show over the logo screen. Video is drawn by mpv under the Slint scene (native spec §5); overlays draw over video like text does.

### 6.2 Text slides (`layout.rs`, `raster.rs`)
- `TextSlide` gains `styles: &[StyleSpan]`; `RenderKey` stays a struct for slides and gains `styles: Vec<StyleSpan>` (FEAT-5 adds `second`). Text stays a plain `String` plus `styles`; scripture and FEAT-5's `ParallelText` always have empty `styles`.
- **Renderer cache key.** The renderer cache is keyed by one enum, defined here: `pub enum CacheKey { Slide(RenderKey), Overlay(OverlayKey), Stage(StageKey) }`. `OverlayKey` is §6.4; FEAT-3 defines `StageKey`. All three share one LRU budget.
- Body: `Buffer::set_rich_text` with one `Attrs` per run: family; weight `theme.weight` for lyrics and announcements (scripture keeps 500), `700` when the run is bold (never above 700); `Style::Italic` when italic (bundled Noto Sans Italic; cosmic-text synthesises for other families); colour from the run or the theme; `metrics(px × size/100)` per run at each fit step. Alignment from `theme.align` (scripture stays left). Position `top` puts the body box at the top padding.
- **Fonts are a hard requirement of rich text**: `NotoSans-Bold.ttf` (700) and `NotoSans-Italic.ttf` (400) join the bundled 400/500/600 faces (about 1 MB, OFL). Without them bold would silently render at 600.
- Underline: a filled rect under each underlined glyph run, from cosmic-text's decoration metrics (fallback 6 % of font size below baseline, thickness 5 %), coloured like the run.
- Outline: body coverage dilated by `outline × scale` px (separable max filter on `Coverage`), composited in `outline_color` under the text and above the shadow.
- Text band: a rounded (12 px) rect around the laid-out lines plus 28 px padding, `band_color` at `band_opacity`, drawn first. With `position = "lower"` it replaces the lower-third gradient.
- Footer and reference lines use the theme colour and font as today.
- Defaults give today's pixels; all goldens unchanged (§9.1).

### 6.3 Backgrounds (`render/background.rs`, new)
`pub fn background_pixels(bg: &Background, decoded: Option<&RgbaImage>, canvas) -> Rendered` draws colour, gradient (tiny-skia `LinearGradient` at `angle`) or a decoded picture (cover fit), then the `dim` black layer. Room `prepare.rs`, Plan 3 thumbnails and FEAT-7 compose call it (FEAT-7's `render::background_picture` is this function). Video backgrounds: mpv draws the video; the `dim` layer is a Slint rectangle (`black` at `dim/100`) above the video in the background `FadeLayer`.

### 6.4 Overlays (`render/overlay.rs`, new)
- The one entry point: `pub fn draw(fonts: &Fonts, slot: &OverlaySlot, text: Option<&str>, theme_font: &str, canvas: (u32, u32), picture: Option<&RgbaImage>) -> Placed`, with `Placed { x: u32, y: u32, img: Rendered }` holding only the bounding box. FEAT-2 and FEAT-7 refer to this as `render::overlay::draw` (earlier drafts said `render_overlay`). FEAT-2 adds its `Timer` and `Message` arms inside it.
- Box width = `place.width`% of canvas width; text wraps inside it (minus 24 px padding each side when boxed) and shrinks from `look.size` down to 18 px to fit at most 3 lines; pictures scale to the box width keeping aspect ratio, at `opacity`. Position: anchor plus `margin × 10` logical px, scaled by `canvas.w / 1920`. `Bar` boxes span the full width at the anchor's edge.
- Text overlays reuse the slide's rich-run path (`layout::shape_runs`, factored out of `layout`) and shadow.
- Caching: overlays are cached under `CacheKey::Overlay` (§6.2). `Renderer` gets `Job::Overlay(OverlayKey)`, `OverlayKey { source, text, place, theme_font, canvas }` (no `inst`, `key` or `name`). Bound overlays key on their computed text.

### 6.5 Room window
- `room.rs`: `Kind::Overlays` and `LayerKey::Overlays(String)` (serde JSON of the slots without `inst` + canvas) join `KINDS`; timeout `STILL` (2.5 s). Prepare renders each slot separately (cache hits for unchanged ones) and returns `Vec<Result<PlacedPic, (key, inst, reason)>>`; errors become `OverlayFailed` events (§5.3) and never fail the plan.
- `ui/room.slint`: `in property <[OverlayPic]> overlays;` with `export struct OverlayPic { key: string, image: image, x: length, y: length, w: length, h: length, shown: bool }`, drawn by a `for` repeater above `logo` and below `black`. Each item animates `opacity` over `OVERLAY_FADE_MS` on `shown`; one being turned off stays with `shown: false` for 400 ms, then is dropped. A slot whose content changes under the same key crossfades in the same 400 ms (old picture fades out while the new one fades in). Positions are logical px (the glue converts from canvas px).
- Bound overlays: the 1 Hz timer replaces the matching `OverlayPic.image` in place.

## 7. UI (extends Plan 3)

### 7.1 Files
| File | New / extended | Contents |
|---|---|---|
| `ui/dialogs/settings-themes.slint`, `src/ui/dialogs/settings_themes.rs` | New; replace Plan 3's `settings-look.slint` content | Themes tab (§3.1): rows, star, Reset, Logo section calling `settings/logo`. The Settings dialog sends only the fields its tabs edit (§5.1 merge rule). |
| `ui/dialogs/theme-editor.slint`, `src/ui/dialogs/theme_editor.rs` | New | Theme editor, stacked over Settings. |
| `ui/dialogs/background-picker.slint`, `src/ui/dialogs/background.rs` | Extended (Task 18) | Sections incl. "Current", gradients, converting entries, Darken slider, Add / Manage. `choices()` gains sections; `selected()` compares including `dim` and falls back to "Current". |
| `ui/dialogs/backgrounds.slint`, `src/ui/dialogs/backgrounds.rs` | New | Backgrounds manager. |
| `ui/rich-edit.slint`, `src/ui/rich_edit.rs` | New shared widget | `RichEdit`: toolbar + `TextInput` inside a `ScrollView` (Plan 3 pattern, height 8 lines) + Formatting chips (`[RangeChip {label, start, end}]` from `richtext::ranges`) + preview strip. `TextInput` (not `TextEdit`) because only it exposes `cursor-position-byte-offset` / `anchor-position-byte-offset` and `set-selection-offsets`. Rust holds spans and the pending style; `edited` → `richtext::diff_edit` + `adjust`. Its own `FocusScope` handles Ctrl+B/I/U. |
| `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs` | Extended (Task 18) | Scrollable with sections Words, Theme, Automation, Stage, Bible (FEAT-1 adds Words' `RichEdit` for announcements and the Theme section; the others come from FEAT-2, FEAT-3, FEAT-5). |
| `ui/operator/service.slint`, `src/ui/operator/service.rs` | Extended (Task 6) | Theme chip and menu; " · own background" subtitle; the own-background question after `service/theme`. |
| `ui/operator/overlays.slint`, `src/ui/operator/overlays.rs` | New | Overlays strip (36 px, horizontal `ScrollView`, collapsed "+ Overlays"); element ids `overlay-chip-<n>`, `overlays-clear-btn`, `overlays-add-btn`. Sends `seen_clear` from the latest `LiveView` with every overlay-on. |
| `ui/dialogs/overlays.slint`, `ui/dialogs/overlay-editor.slint`, `src/ui/dialogs/overlays.rs` | New | Manager and editor. |
| `ui/dialogs/services.slint` | Extended (Task 19) | Theme name in rows. |
| `ui/room.slint`, `src/ui/room.rs`, `src/ui/room/prepare.rs` | Extended | Overlay repeater, per-slot prepare, `OverlayFailed`, background `dim`, gradient. |
| `src/ui/preview.rs`, `LiveMonitor` | Extended (Task 9) | Same overlay inputs as `RoomWindow`. |
| `src/ui/operator/keys.rs` | Extended (Task 10) | Implements the key table (§7.6). `F6` → `KeyAction::Clear(Command::ClearOverlays)` in the capture set with F1–F5. New `KeyAction::PassThrough`: `route("b", ctrl, _, ctx)` with `ctx.dialog_open` returns `PassThrough`, and the `key` callback returns **false** (not accepted) for it, so the key reaches the focused field (RichEdit). Plan 3's test `ctrl_b_ignored_while_a_dialog_is_open` becomes `ctrl_b_passes_through_while_a_dialog_is_open` ("Bible box not focused; key passes to the field"). Help gets "F6 — Clear overlays". |
| `web/remote.html`, `web/remote.js` | Extended | Overlay chips and Clear overlays; sends `seen_clear` with overlay-on and shows "The screen was cleared — try again." on refusal. |

All colours, radii and fonts in new `.slint` files come from the `Theme` global (Plan 3 rule). Name clash note: the Slint `Theme` global is the operator's own UI theme; slide themes are `model::Theme`/`SavedTheme` and never imported into `.slint`. Data colours (swatches, gradients) come from Rust. Every control a test touches has an element id.

### 7.2 Operator layout budget at 1366×768
This is the one layout budget for all FEAT specs; each spec states how its UI fits it.

**Live column** (Plan 3: 360–440 px wide), top to bottom:

| # | Row | Owner | Budget |
|---|---|---|---|
| 1 | Monitor (with transport and clear grid as Plan 3); FEAT-7's "Room shows" chip sits inside the monitor's top edge | Plan 3 / FEAT-7 | as Plan 3 |
| 2 | Preview | Plan 3 | as Plan 3 |
| 3 | Section line | FEAT-4 | one row |
| 4 | Change banner (only when needed; it also carries theme changes, §5.3) | FEAT-4 | one row, hidden otherwise |
| 5 | **Overlays strip** | FEAT-1 | 36 px |
| 6 | Automation strip | FEAT-2 | at most 2 rows at 768 px height; the rest goes to More… |
| 7 | Stage strip | FEAT-3 | one 32 px row; a single **Stage…** button at 1250 px wide or less |
| 8 | Up next | Plan 3 | the remaining height |

The Overlays strip never wraps; chips are 28 px tall, at most 140 px wide with ellipsis; F6 and + stay pinned at the right edge outside the scrolling area.

**Narrow windows.** Below 1500 px wide: the top-bar **Automation**, **Practice** and **⋯ More** buttons become icon-only (with tooltips), FEAT-5's second-translation chip folds into the translation chip's menu, and the slides header collapses **Choose/Print**, **Slide breaks** and **Custom order** into one **⋯** menu.

**Dialogs.** Item edit and Services become scrollable, with sections **Words**, **Theme**, **Automation**, **Stage** and **Bible** (each shown only when it applies to the item kind), and fit in 768 px height.

### 7.3 Strings
User-visible core messages live in `src/strings/themes.rs` (`pub const`s, plus `fn`s for formatted ones like `theme_exists(name)`, `overlay_failed(name, reason)`, `own_backgrounds(n)`), used by `app.rs`, `live.rs` and the UI glue. Literal labels in new `.slint` files use `@tr("…")`. Wording follows `web/operator.js`: short, plain, second person. UI words (all specs): "theme" (slide styles), "background", "overlay" (never "prop"), and "screen view" (what each output shows, FEAT-7) — never "layer" or "z-order". "Look" and "Props" are not used, in the UI or in new identifiers.

### 7.4 Bundled fonts
Required: Noto Sans 400/500/600 (today), **700 and Italic 400** (new, required by §6.2). Optional extra families are decision 21.

### 7.5 Settings tabs (the single table)
Every FEAT spec adds its Settings tab here and nowhere else.

| Tab | Owner | Contents |
|---|---|---|
| **Themes** (renamed from Plan 3's Look) | FEAT-1 | Theme list, star, editor, Logo (§3.1) |
| Presenting | FEAT-4 | **Section keys** checkbox (FEAT-4) |
| Songs | Plan 3 / FEAT-7 | Existing song options; FEAT-7 adds its song settings (CCLI usage) here |
| Bible | Plan 3 / FEAT-5 | Existing options; FEAT-5 adds per-translation status and keys |
| Video | Plan 3 | Existing |
| **Stage** | FEAT-3 | Stage layouts and stage display options |
| **Planning Center** | FEAT-7 | Connection and sync |
| Backups | Plan 3 | Existing |
| Help | Plan 3 | Existing (gets "F6 — Clear overlays" and the other new keys, §7.6) |
| About | Plan 3 | Existing |

Each tab sends only the fields it edits (§5.1 merge rule).

### 7.6 Keyboard shortcuts (the single table)
Every FEAT spec references this table; every binding is checked against FEAT-3's `OS_RESERVED`. Clear-key lists in every spec read "F1–F6". This amends v1 §7.3.

| Scope | Keys | Owner |
|---|---|---|
| **Capture set** (works while typing and over dialogs) | F1–F5 (Clear All, Clear Slide, Clear Media, Clear Audio, Logo) | v1 |
| | **F6** Clear overlays | FEAT-1 |
| | Ctrl/Cmd+Shift+M Show message | FEAT-2 |
| | Ctrl/Cmd+Shift+J Hide stage message (consumed only while a stage message is up) | FEAT-3 |
| **Global, not over dialogs** | Ctrl/Cmd+B (existing; passes through to the focused field when a dialog is open, §7.1) | Plan 3 |
| | Ctrl/Cmd+J focuses the stage message box | FEAT-3 |
| **Presentation scope** (not in a text field, no dialog open) | Arrows, Space, Enter | v1 |
| | Bare 1–9 and section letters: jump within the live song | FEAT-4 |
| | Alt+N: stage note on the selected slide (bare N stays free) | FEAT-3 |
| | Alt+digit translation chips (existing) | FEAT-5 |
| **Inside the rich text editor** | Ctrl+B / Ctrl+I / Ctrl+U | FEAT-1 |

FEAT-6 and FEAT-7 add no keys.

## 8. Errors (toasts unless noted)
| Situation | Message |
|---|---|
| Theme without a name / duplicate / reserved | "Give the theme a name." / "There's already a theme called “<name>”." / "That name is used by a built-in theme. Pick another." |
| Theme gone (stale menu) | "That theme is gone." |
| Delete a built-in | "Built-in themes can't be deleted." |
| Background import of a non-picture/video | "Backgrounds can be pictures or videos." |
| Background video conversion failed | entry shows "Couldn't convert" (not a toast) |
| Background picture unreadable on air | existing room message; the room keeps what it showed |
| Overlay without name / text | "Give the overlay a name." / "Type the overlay's text." |
| Overlay gone | "That overlay is gone." |
| More than 8 overlays | "Up to 8 overlays can be on at once. Turn one off first." |
| Logo bug with no logo set | "Choose a church logo first (Settings → Themes)." |
| Overlay picture unreadable | Live notice "Couldn't show the overlay “<name>”: the picture could not be loaded." — the overlay is taken off, slides keep working |
| Logo picture gone | "That picture is gone." |
| Newer data | banner + every edit: "This data was saved by a newer RustyPresenter. Please install the latest version." |
| Font not installed | note in the Theme/Overlay editor |
| Live item's theme changed | FEAT-4's change banner (`LookOnScreen`) with **Update screen** (not a toast) |
| Overlay turned on after a clear the sender hadn't seen | "The screen was cleared — try again." |
| Overlay edited while on | chip dot + tooltip "Changed — turn it off and on to update." |
All errors are recoverable; none clear the room.

## 9. Testing

### 9.1 Unit tests (core, `cargo test`)
- `theme::tests`: `item_theme_beats_service_and_default`, `service_theme_beats_default`, `dangling_item_theme_falls_through`, `theme_without_background_uses_standard_background_not_service_theme`, `item_background_beats_theme_background`, `deck_and_media_items_ignore_theme`, `starred_theme_is_church_default`, `dangling_default_falls_back_to_standard`, `builtin_theme_resolves_and_override_wins`, `service_of_finds_item_in_unselected_service`.
- `model`/`store`: `old_data_json_loads_with_defaults` (v0.2 fixture), `migrate_adds_library_entries_for_used_backgrounds`, `migrate_names_entries_from_media`, `migration_saves_immediately_and_does_not_rerun`, `migration_makes_labeled_backup_first`, `migration_with_missing_asset_still_loads` (falls back to `data.before-update-*.json`), `labeled_backup_survives_prune`, `newer_format_opens_read_only`, `after_load_is_the_only_hook_and_saves_when_it_returns_true`, `migration_chain_backs_up_before_each_step`, `background_entry_added_is_rfc3339`, `newer_format_unparseable_still_starts_read_only`, `old_live_state_json_loads`, `unknown_overlay_source_loads_as_unknown`, `referenced_assets_include_themes_backgrounds_overlays`, `referenced_assets_plus_live_frame_covers_deleted_overlay` (the helper future cleanup must use).
- `backgrounds::tests`: `builtin_backgrounds_match_plan3_swatches_and_six_gradients`.
- `richtext::tests`: `normalize_snaps_to_char_boundaries`, `normalize_merges_and_splits_overlaps`, `apply_bold_to_middle_splits_span`, `clear_formatting_removes_only_range`, `typing_inside_span_extends_it`, `typing_at_span_end_inherits_style`, `delete_across_spans_shrinks_both`, `slice_by_blank_line_blocks_rebases_offsets`, `uppercase_keeps_spans_aligned_for_eszett`, `diff_edit_finds_middle_insert`, `ranges_label_cuts_long_text`.
- `slides::tests`: `text_slides_carry_sliced_styles`, `text_slides_without_styles_unchanged`.
- `live::tests`: `overlay_on_bumps_and_is_idempotent`, `overlay_on_changed_copy_replaces`, `overlay_off_when_absent_is_noop`, `ninth_saved_overlay_is_rejected`, `cap_ignores_non_saved_slots`, `overlays_survive_next_show_and_clears_except_all`, `logo_removes_timer_keeps_bug_and_message` (timer/message slots built directly), `logo_keeps_text_and_clock`, `clear_all_removes_overlays`, `clear_overlays_noop_when_none`, `overlay_commands_need_no_base`, `overlay_on_with_stale_seen_clear_is_refused`, `overlay_on_without_seen_clear_is_refused`, `overlay_off_and_clear_overlays_ignore_seen_clear`, `every_clear_and_logo_bumps_clear_epoch_even_when_nothing_removed`, `clear_epoch_survives_restore`, `deleted_overlay_stays_on_screen_until_off`, `show_overlay_and_hide_overlay_do_not_bump`, `logo_bug_follows_logo_change_on_next_toggle`, `logo_bug_without_logo_errors`, `overlay_failed_removes_slot_and_sets_notice`, `stale_overlay_failed_report_ignored`, `missing_overlay_picture_does_not_block_next`, `restore_with_missing_overlay_asset_shows_slides`, `live_item_snapshots_resolved_theme`, `theme_edit_does_not_change_frame`, `show_after_theme_change_applies_new_theme`, `next_within_item_keeps_snapshot_theme`.
- `app::tests`: `theme_save_requires_unique_name`, `theme_name_church_default_reserved`, `theme_delete_clears_references_and_counts`, `delete_starred_theme_returns_to_standard`, `make_default_keeps_previous_default_as_theme` (star Christmas, then star Standard → original look back, Christmas unchanged), `editing_starred_theme_changes_default`, `theme_save_standard_writes_only_theme_and_background`, `service_theme_rejects_unknown`, `service_theme_reports_items_with_own_background`, `clear_item_backgrounds_only_that_service`, `service_update_normalizes_styles`, `service_update_without_theme_key_keeps_theme`, `service_update_without_styles_key_keeps_styles`, `overlay_save_validates`, `background_import_purpose_adds_entry_not_media`, `background_video_needing_conversion_becomes_ready`, `logo_choose_saves_immediately`, `settings_save_keeps_logo_set_meanwhile`, `settings_save_keeps_theme_background_changed_meanwhile`, `settings_save_missing_key_keeps_value`, `settings_save_null_clears_optional_field`, `service_update_missing_keys_keep_every_new_field`, `theme_save_returns_empty_screen_views`, `theme_change_on_live_item_sets_look_on_screen` (through FEAT-4's reconcile; no FEAT-1 flag), `theme_change_on_item_no_longer_found_sets_nothing`, `read_only_rejects_edits`.
- `server::tests`: `remote_may_toggle_overlays`, `remote_overlay_on_with_old_seen_clear_says_screen_was_cleared`, `remote_next_prev_gotocue_takelive_need_base`, `remote_may_not_save_overlays_or_themes`, `remote_view_lists_saved_overlays_with_state`.
- `room::tests`: `overlay_change_prepares_only_overlays`, `failing_overlay_slot_reports_and_commits_other_layers`, `church_default_edit_does_not_replan_slide_with_snapshot_theme`, `clock_tick_does_not_need_a_plan`.
- `keys::tests`: `ctrl_b_passes_through_while_a_dialog_is_open`, `f6_clears_overlays_in_capture_set`.
- `render` goldens (bundled fonts only, `Fonts::bundled_only()`): **all existing goldens unchanged** (`default_theme_pixels_unchanged`, including scripture at 500), plus `bold_italic_underline_runs`, `bold_run_in_semibold_theme_is_700`, `colored_run`, `large_run_still_fits`, `align_left_right`, `position_top`, `outline_thick`, `text_band_lower`, `gradient_background_135`, `dim_50_image_background`, `overlay_text_bar_bottom`, `overlay_picture_top_right_scaled`, `overlay_clock_box`, `overlay_on_4x3_keeps_corner`, `overlay_draw_is_bounding_box_only`, `cache_key_keeps_slide_and_overlay_entries_apart`.

### 9.2 UI tests (Slint testing backend; `tests/ui_operator_flows.rs` and per-dialog tests; `Ops::inline`)
`themes_tab_lists_standard_first_with_star`, `new_theme_saves_and_appears`, `star_asks_then_moves_default`, `delete_theme_asks_with_counts`, `theme_editor_preview_rerenders_after_change` (mock time 150 ms), `missing_font_note_in_theme_editor`, `logo_section_saves_without_settings_save`, `service_theme_chip_picks_theme`, `service_theme_asks_about_own_backgrounds`, `item_edit_theme_combo_saves`, `background_picker_sections_and_darken_slider`, `picker_keeps_removed_library_background`, `converting_background_not_pickable`, `add_background_imports_to_library`, `rich_edit_bold_selection_then_save_round_trips`, `rich_edit_lists_formatted_ranges`, `rich_edit_chip_x_clears_range`, `rich_edit_ctrl_b_bolds_in_dialog_not_bible_box`, `rich_edit_b_disabled_in_bold_theme`, `rich_edit_pending_style_applies_to_typed_text`, `overlay_chip_toggles_and_lights`, `overlays_strip_collapses_when_empty`, `f6_clears_overlays_while_typing`, `overlay_edit_while_on_shows_changed_dot`, `overlay_failed_darkens_chip_and_shows_notice`, `theme_change_on_live_item_shows_feat4_banner_and_update_screen_applies_it`, `overlays_strip_fits_live_column_budget_at_1366x768`, `settings_dialog_sends_only_edited_fields`.

### 9.3 Manual checks (each platform, real projector where possible)
1. Make "Christmas" (red/gold gradient, Bold, outline) → pick it for one service → songs and scripture show it; another service still shows the church default.
2. Star "Christmas" → every service without a theme uses it; star "Standard" → the old look is back.
3. Change the church default while a song is live → room unchanged; FEAT-4's change banner shows at once; **Update screen** → new look on the same slide.
4. Announcement with a gold bold date and a Huge heading → room matches the preview strip; the Formatting chips list both.
5. Logo bug (church logo, top-right, 12 %) + clock bottom-left → step through songs, a video, a deck and Logo → overlays stay; F1 and F6 remove them; phone toggles them. With a FEAT-2 countdown (when available): Logo removes it.
6. Picture overlay on, then unplug and replug the room monitor after deleting the picture file → the room comes back with slides; the overlay is taken off with the notice; Next works.
7. A 4:3 projector and a **1366×768 laptop** as operator and room: the Overlays strip stays one row with 10 overlays (scrolls); overlays keep their corners; text bands fit.
8. Old laptop (integrated GPU, software fallback): a clock over a playing video, under 5 % extra CPU.
9. Restore a pre-FEAT-1 backup → it loads, migrates once (one "before-update" backup across three restarts); backgrounds appear in the library.
10. Kill the app with overlays on → after restart the overlays and themed slide are back.
11. Import a background video that needs conversion → "Converting N %" in the picker, then pickable.

## 10. Dependencies on other FEAT specs
- **FEAT-2**: adds `OverlaySource::Timer { timer_id, caption, large, look }` and `Message { shown, template_id, text, look }`, keys `timer:<uuid>` / `message`, z values as §4.5, and calls `show_overlay` / `hide_overlay` / `retain_overlays` from its `mutate` (they don't bump; FEAT-2's `commit_change` does). It uses FEAT-1's final names (§4.5): `OverlaySlot`, `OverlaySource`, `SlotKey`, its own `timer_key(id)` and `MESSAGE_KEY`; `OverlayOn`, `OverlayContent` and `MESSAGE_SLOT` are retired. Its `ShowMessage`, timer show and macro show steps carry `seen_clear` and its macro Clear/Logo steps bump `clear_epoch` (§5.2). It owns the only phone allowlist table (§5.11), which lists FEAT-1's rows. Its time types follow §4.3 (chrono only). Its automation seed runs in `Data::after_load` (§4.7). Its `OverlayPlace`/`MessagePlace` map to `Placement` (§4.5); its message crossfade uses `OVERLAY_FADE_MS` (400 ms, not 300). Its tick is `Update::Timers`; FEAT-1 has no tick update. **Change FEAT-2 must take:** Timer overlays are **removed by Logo** (its §5.5 row and test `logo_keeps_timer_and_message` → `logo_removes_timer_keeps_message`); FEAT-1's combined matrix (§5.2) is authoritative. Timers/messages don't count toward the 8 cap. Its pre-service macro uses `Command::Overlay` only for saved overlays.
- **FEAT-3**: stage layouts reference timers by id; the stage never shows the room's overlay layer. It defines `StageKey` for FEAT-1's `CacheKey` (§6.2), adds its Stage tab to §7.5, its keys to §7.6 and its Stage strip to the §7.2 budget; `live_notes` is dropped in favour of FEAT-4's reconcile.
- **FEAT-4**: section colours are operator-only. FEAT-4 owns the one edited-live-item mechanism (reconcile, `Live.changed`, change banner, `Command::Refresh`); a theme difference against `LiveItem.theme` is its `LookOnScreen` reason (§5.3). Its section keys and Presenting tab are in §7.5/§7.6; its section line and banner in §7.2.
- **FEAT-6**: copy-last-week and templates copy `Service.theme`; print/export uses `theme::resolve` and `render`. Practice routing follows FEAT-6 §5.5 (§5.2 here): `Overlay { on: true }` goes to the practice engine; `ClearOverlays` (F6), `Overlay { on: false }` and every other clear reach both engines, so a clear always reaches the room (F1 included; FEAT-6 decision 16 is reversed). FEAT-6 sets `SUPPORTED_FORMAT` to 3 (§4.7). Its "Show the logo, then practice" hides timer overlays because Logo removes them (§5.2).
- **FEAT-7** exports it may use: `theme::ThemeId`, `theme::find`, `theme::builtin()` (FEAT-7's `STREAM_LOWER_THIRD_THEME` lives there with a fixed id and is never seeded into `Data.themes`; editing stores an override, Delete is offered as **Reset**), `theme::resolve`, `LiveItem.theme` / `SlideLayer::Text.theme`, `Frame.overlays: Vec<OverlaySlot>` with `OverlaySource` (FEAT-7's `OverlayClass { Saved, Timer, Message }` and `class_of(slot: &OverlaySlot) -> OverlayClass` match on `slot.source`; `Unknown` counts as `Saved`), `render::overlay::draw`, `render::background::background_pixels` (its `background_picture`), `overlay::bound_text`, room `Kind::Overlays` above the logo. Hooks FEAT-7 adds to FEAT-1 actions: `theme/delete` asks `screen_view::theme_in_use` and refuses with its reason; `theme/save` already returns `{id, screen_views: [..]}` (§5.1) and FEAT-7 fills `screen_views`. Its screen-view and usage cleanup runs in `Data::after_load` (§4.7); its Songs and Planning Center tabs are in §7.5; its "Room shows" chip is in the §7.2 budget. The user word for slide styles is **theme**; FEAT-7 uses **screen view**; "Look" and "Props" are not used.
- **FEAT-5**: adds `second` to `RenderKey` (§6.2); scripture and `ParallelText` have empty `styles`; its fix-ups run in `Data::after_load`; its `slides::index_for_source` is FEAT-4's scripture mapper for **Update screen**; its Bible tab content and Alt+digit chips are in §7.5/§7.6; `live.notice` re-take is dropped.
- **Plan 3** must be merged first (Settings, Item edit, background picker, Live column, keys, `ScrollView` pattern). **Plan 4** (mpv probe) gives video background thumbnails.

## 11. Build order
1. Model + `SUPPORTED_FORMAT` + migration chain + `Data::after_load` + format guard + labelled backups + `richtext` + `theme` (resolve, default reference, built-ins) (core tests). 2. Live engine (theme snapshot, slots, mutators, Logo policy, commands, `clear_epoch`/`seen_clear`, `OverlayFailed`). 3. Render (`CacheKey`, styles, bold/italic faces, outline, band, gradients, dim, `overlay::draw`) with goldens. 4. Room + preview overlay drawing with per-slot prepare. 5. Actions (incl. `settings/logo`, the merge-by-key rule for `settings/save` and `service/update`, `theme/save` → `{id, screen_views}`, background conversion target) + strings. 6. Themes tab + Theme editor. 7. Background picker + manager. 8. Rich editor + scrollable Item edit (Words, Theme sections) + `keys.rs` (key table §7.6, pass-through). 9. Overlays strip, manager, editor, F6. 10. Phone remote. 11. Manual checks; update `PROJECT_PLAN.md` (including rewording §10b's FEAT-1 row to the final words: "Themes, backgrounds and overlays" — theme per item/service, rich text, background library, overlays that stay up while slides change, and "screen views" (FEAT-7) for which layers each output shows; no "looks" or "props"); update the v1 spec: §6.2 (new F6 column, the combined per-source overlay rows, and the Logo bullet: "Logo clears all visual layers except overlays whose source survives the logo"), §6.3 (incoming slides leave overlays alone) §7.3 (keys: point to §7.6 here) and §12.3 (phone allowlist: point to FEAT-2 §5.11, which adds `Overlay` and `ClearOverlays`; base is required for Show/Next/Prev/GoToCue/TakeLive/Jump, not only Show).

## 12. Decisions for Jake to review
Each line: **default chosen** — main alternative.
1. **Theme changes don't restyle what's on screen until the operator clicks Update screen on FEAT-4's change banner (or clicks a slide of the live item, or moves to the next item)** (live safety; changed Nov 1 from FEAT-1's own "Theme changed — click the live slide" line to FEAT-4's shared banner) — apply instantly like v0.2.
2. **Resolution item > service > church default, and only the chosen theme counts** (a theme without a background uses the Standard background) — merge field by field across levels.
3. **Themes can be picked per service and per item; songs don't remember a theme** — let a song carry a default theme too.
4. **The church default is a star on one theme (a pointer), and today's style stays as an undeletable "Standard" theme** — copy a theme's settings into the default (loses the old default; copies drift apart).
5. **Settings tab renamed "Look" → "Themes"**, logo moved into it and saved immediately — keep "Look" with a Save button.
6. **UI word "Overlays"** (code: overlay); "Props" and "Look" are not used anywhere, in the UI or new identifiers (settled across all specs Nov 1) — "Props" (ProPresenter's word) or "Extras".
7. **Logo (F5) keeps pictures, clocks, text signs and messages, but removes timers**; Clear All and F6 remove everything — Logo removes all overlays, or keeps all of them (timers too).
8. **Keep FEAT-1 Text overlays as "signs that stay up" (no fields, anywhere on screen, rich text) alongside FEAT-2 messages ("notices" with fields, banner, auto-hide)** — drop Text overlays and use FEAT-2 messages for all text (one way, but nothing until FEAT-2 ships and no rich text).
9. **New key F6 = Clear overlays**; no per-overlay hotkeys — Ctrl+1…9 to toggle overlays.
10. **Up to 8 saved overlays on at once; FEAT-2 timers and messages don't count** — one shared limit, or no limit.
11. **Phone remote can turn overlays on/off and clear them**, nothing more; turning one on after a clear the phone hadn't seen is refused with "The screen was cleared — try again." (changed Nov 1) — phone can't touch overlays.
12. **Overlay edits don't update the screen until turned off and on** — live-update overlays when saved.
13. **A picture overlay set to "the church logo" follows the logo when next turned on** — store the picture at the moment it was chosen.
14. **An overlay that can't be shown is taken off with a notice; slides keep working** — keep it in the list and retry.
15. **Rich text only in announcements and text overlays** (not lyrics or scripture) — allow it on song slides too.
16. **Editor shows plain text plus a list of formatted ranges and a live preview** (Slint has no editable rich text) — a markup syntax like `**bold**`, or marks drawn over the text (fragile).
17. **No undo for formatting changes** (Cancel discards everything) — a formatting undo stack.
18. **Run sizes: Small 75 %, Normal, Large 125 %, Huge 150 %** — free size in px.
19. **New theme options: weight, align, top position, outline, text band; weight applies to songs and announcements, scripture stays at its current weight** — fewer options, more options (letter spacing, line height), or a separate scripture weight.
20. **Bold is capped at Bold (700); in a Bold theme the B button is disabled** — synthetic extra-bold.
21. **Noto Sans Bold and Italic are bundled (required); extra families (Noto Serif Regular/Bold, Montserrat SemiBold/Bold, Oswald Medium; about 2 MB, all OFL) are also bundled and marked "works on every computer"** — only the required Noto Sans faces.
22. **Six built-in gradients, no bundled photos or motion videos** (small installer, no licensing work) — ship a starter pack of free CC0 backgrounds.
23. **Background library is separate from the media library**; "Add picture or video…" adds to backgrounds, media pictures are still offered — one shared library with a "use as background" flag.
24. **Background videos are converted like media videos and can't be picked until ready; the original file isn't kept** — keep the original too (more disk).
25. **Removing a background from the library never changes items that use it** — also reset those items to the theme background.
26. **Picking a service theme with its own background asks whether items with their own background should switch too** — always switch them, or never ask.
27. **Darken slider 0–80 % per background use** — a theme-wide darken setting.
28. **Deleting a theme sends its items and services back to the church default** (after a question with counts) — refuse while in use.
29. **Duplicate theme names are not allowed; "Standard" and "Church default" are reserved** — allow them.
30. **Data from a newer version opens read-only with a message** — open it normally and risk losing the newer fields on save.
31. **Overlays are hidden on the stage screen** — show them there too (FEAT-3/FEAT-7 can add this).
32. **Clock overlay: 12-hour without seconds by default** — 24-hour or with seconds.
33. **A clear always wins over a delayed show** (new Nov 1): an overlay, message or timer turned on by someone who hadn't yet seen the latest clear is refused with "The screen was cleared — try again."; turning things off is always allowed — let the late show through.
34. **In practice mode, turning an overlay on goes to practice, but F6, overlay off and every clear also clear the room** (new Nov 1, follows FEAT-6 §5.5) — clears go only to practice too.
35. **Below 1500 px wide, top-bar buttons become icons and the slides header folds into a ⋯ menu; the Stage strip becomes one Stage… button at 1250 px or less** (new Nov 1, §7.2) — let the Live column scroll instead.

## 13. Review notes
- P1 overlay model: fixed as a generic slot layer. One deviation from the suggested shape: `look` stays inside the text-bearing source arms (as FEAT-2 already writes them) rather than on the slot, because Picture sources have no look.
- P1 Logo policy: FEAT-2's current text says timers stay on Logo; this spec follows the review and FEAT-6 (timers removed) and records the change FEAT-2 must take in §10.
- P1 make-default: chose fix (a), a reference with an undeletable "Standard", over (b) auto-saved "Previous default" copies, because (a) also removes the drift problem. The suggested test name `make_default_keeps_previous_default_as_theme` is kept and tests the reference behaviour.
- P2 fade length: one constant (400 ms) for show, hide and in-place replacement; FEAT-2's 300 ms crossfade becomes 400 ms.
- P2 bold at 700: chose disabling the B button over synthetic emboldening (simpler, nothing that looks different on the projector from the preview).
- FEAT-7 built-in theme: registry entries are read-only in code but can be overridden and Reset, which satisfies both the review (read-only registry) and FEAT-7's "editable with Reset".

## 14. Cross-spec alignment (Nov 1)
Applied the cross-spec rulings; what changed in this spec:
- **Overlay names final** (§4.5, §10): `OverlaySlot`, `OverlaySource` (FEAT-1 owns it; FEAT-2 adds `Timer`/`Message`), `SlotKey` with `saved_key`/`timer_key`/`MESSAGE_KEY`. `OverlayOn`, `OverlayContent`, `MESSAGE_SLOT` retired; the "maps to" note in §10 is gone. FEAT-7 uses `OverlayClass` + `class_of` matching on `slot.source`.
- **Time types** (§4.3): `BackgroundEntry.added` is `chrono::DateTime<Utc>` (RFC 3339), not a `now_stamp` string; chrono only.
- **Clear epoch** (§4.6, §5.2, §5.6, §5.7, §3.6, §3.7): `Live.clear_epoch` bumped by every clear (F1–F6, phone, macro Clear/Logo); `Overlay { on: true }` carries `seen_clear` and is refused when stale ("The screen was cleared — try again."); off and clear commands stay baseless. New decision 33.
- **Practice routing** (§5.2, §10): overlay on goes to the practice engine; F6, overlay off and every clear reach both engines. New decision 34.
- **Format and load hook** (§4.7, §4.8): `store::SUPPORTED_FORMAT` is the only constant (FEAT-1 → 2, FEAT-6 → 3); the migration chain lives in `store::load` with a before-update backup per step; `Data::after_load(&mut self) -> bool` is the only fix-up hook.
- **Built-in stream theme** (§3.1, §4.2, §5.1, §10): built-ins live only in `theme::builtin()`, never seeded (no `theme::seeded`); Delete is offered as Reset; `theme/save` returns `{id, screen_views}`.
- **UI words and Settings tabs** (§7.3, §7.5, §11): "theme", "overlay", "screen view"; no "Look"/"Props". New single Settings tab table (§7.5); PROJECT_PLAN §10b reworded at build step 11. Decision 6 updated.
- **Merge semantics** (§5.1, §7.1): `settings/save` and `service/update` merge by key (missing keeps, `null` clears) for every new Service/ServiceItem field; Plan 3's "keep live fields" copy list is replaced.
- **Edited live item** (§3.2, §4.6, §5.1, §5.3, §5.7, §7.1, §8, §9, §9.3): `theme_stale`, its rebroadcast and the "Theme changed" line are dropped; a theme difference is FEAT-4's `LookOnScreen` with **Update screen**; `LiveItem.theme` is read by reconcile; `live.notice` is short-lived. Decision 1 updated.
- **RenderKey / CacheKey** (§6.2, §6.4): `CacheKey { Slide, Overlay, Stage }` defined in §6.2; `RenderKey` gains `styles` (FEAT-5 adds `second`); scripture/ParallelText styles are empty.
- **Phone allowlist** (§3.7, §5.6, §11): references FEAT-2 §5.11; base required for Show/Next/Prev/GoToCue/TakeLive/Jump, not only Show. Decision 11 updated.
- **Key table** (§7.1, §7.6): new single keyboard-shortcut table; fixed the §7.4 decision reference (16 → 21).
- **Layout budget** (§7.2, §3.3, §3.6): one Live-column order and narrow-window rules; Item edit is scrollable with Words/Theme/Automation/Stage/Bible sections. New decision 35.
- Tests (§9) and build order (§11) updated to match.
- Open: the struct `OverlayLook` and the `look` field on text-bearing overlay sources (shared with FEAT-2's `Timer`/`Message` arms) still contain the word "look". They name an overlay's font/colour/box, not the retired UI word, and were left unchanged here because FEAT-2 uses them too; if the no-"Look"-in-identifiers rule is meant to cover them, rename both specs together (e.g. `OverlayTextStyle` / `text_style`).
