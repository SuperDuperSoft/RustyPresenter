# Native App — Plan 3: Operator Window and Dialogs

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The native operator window, rebuilt 1:1 from today's `web/operator.html` / `operator.css` / `operator.js`. It has the same layout, features, wording and dark theme (as a Slint `Theme` global), every dialog and every keyboard shortcut. Files can be added through native `rfd` dialogs or dropped from the desktop. Settings → About shows `AboutSlint` and the THIRD-PARTY-LICENSES file. The live preview shows video with mpv `screenshot-raw` at 4 Hz, never through a second player. Testing-backend UI tests cover the main flows. At the end, `rustypresenter-native` opens the native operator instead of the web page in a browser.

**Architecture:** New Rust glue lives under `src/ui/operator/` (the main window, one file per column or concern) and `src/ui/dialogs/` (one file per dialog). New `.slint` files live under `ui/operator/` and `ui/dialogs/`, with shared `ui/theme.slint`, `ui/widgets.slint` and `ui/icons.slint`. Every action goes through `App::do_action` in-process using `ui::ops::Ops`: `slint::spawn_local(async move { let r = rt.spawn(core_call).await; done(r) })` runs the core call on tokio and runs `done` on the UI thread, so `done` needs no `Send` bound and may hold `Rc<Operator>`, `RefCell` state and `Rc<VecModel>`. The window follows `App::subscribe()` the same way `ui::stage` does. A plain thread turns each `Update` into a `Send` snapshot (`Arc<OperatorSnapshot>` for `Update::Data`, `LiveUpdate` for `Update::Live`) and only the newest one is applied on the event loop. Logic that does not need Slint (subtitles, Up next text, key routing, grid columns, insert position, selection fallback, file routing) lives in pure functions in `src/ui/operator/view.rs` and `keys.rs`, and is unit-tested without a window. Dialogs are **in-window overlays** (scrim + card + their own `FocusScope`), because Slint 1.18 has no modal or owner-window API (native spec §6). Confirms and prompts are small overlays stacked on top of them. The room, stage, video and monitor code from Plan 2 is reused unchanged, except for one addition: a `FrameGrabber` for the live preview.

**Tech Stack:** Rust 2024, Slint 1.18.1 (`backend-winit`, `renderer-femtovg`, `renderer-software`, `unstable-winit-030`, plus new `raw-window-handle-06`), `rfd` (async dialogs; exact version pinned in Task 15), `arboard = "=3.7.0"` (already in `Cargo.lock`), `i-slint-backend-testing` 1.18, `cargo-about` (dev tool, for THIRD-PARTY-LICENSES), existing `render`, `video`, `room`, `platform`, `slides`, `bible` modules, existing `qrcode` (SVG) shown with `slint::Image::load_from_svg_data` (SVG support is already on through the winit/femtovg backends; no extra feature).

**Spec:** `docs/superpowers/specs/2026-10-29-native-slint-design.md` §3, §4 (fonts), §5 (live preview, missing player), §6, §7, §10, §12 (C3, C4), §13. The behaviour reference is `web/operator.js`, `web/operator.html` and `web/operator.css` (the native spec says "same layout and features as today"); line numbers below refer to those files. The original v1 spec is `docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md`; §7.1–7.3 (navigation and focus), §8.1–8.2 (Bible box) and the §14.2 "Navigation" acceptance row still apply. **Check results:** `docs/superpowers/checks/2026-native-checks.md` (C3 not run yet; C4 PASS on Windows).

**Prerequisite:** Plan 2 Tasks 8–9 are merged before this plan starts. Tasks 1, 5, 9 and 23 depend on `src/bin/native.rs`, `src/ui/screens.rs` and `NativeScreens`, which do not exist until then (today `src/bin` is missing and `src/ui` has only `room.rs` and `stage.rs`).

## Global Constraints

- All Plan 1 and Plan 2 constraints still apply. The Tauri app builds (`cargo build`) and `cargo test` passes after every task, and after every `Cargo.toml` change. No blocking work runs on the UI thread: `do_action`, `OperatorSnapshot::read`, Bible lookup, file I/O, image decoding, slide rendering and `screenshot-raw` all run off it.
- **1:1 with the web operator.** Every string in this plan is copied from `web/operator.js` / `.html`, and every size or colour from `operator.css`. Where the web and the v1 spec disagree, keep the **web** behaviour. The only exceptions are listed under "Product decisions" below.
- **One command surface.** The operator calls only `App::do_action` (through `Ops`), `App::command` (through `Ops::cmd`, so the command id and `base` rules stay the same) and the new `App::import_path` (Task 2). It never edits `App.data` directly.
- **Command rules (from `operator.js:79-89`).** `next`/`prev` send `base = live.version`. `show`, the clears, `logo`, `media_pause` and `audio_pause` send no base. Every command gets a fresh `Uuid` id. When `Outcome.ok` is false, an error starting with `stale` is silent, "End of service" and "Start of service" are normal toasts, and anything else is an error toast.
- **Toasts:** 2.6 s, or 6 s for errors. Every `Ops` failure becomes an error toast with the core's message unchanged (the `guard()` rule).
- **Theme:** every colour, radius and font in the **new** operator and dialog `.slint` files (`ui/operator*.slint`, `ui/operator/`, `ui/dialogs/`, `ui/widgets.slint`, `ui/icons.slint`) comes from the `Theme` global (Task 3). No hex literals in those files. Existing files (`ui/stage.slint`, `ui/room.slint`) keep their literals in this plan.
- **Element ids:** every control a test touches has an element id (`OperatorWindow::next-btn`, etc.). `build.rs` already compiles with `with_debug_info(true)`. Tests use `ElementHandle::find_by_element_id`, never accessible labels (C4 finding).
- **Testing backend:** Slint's platform slot is thread-local and cargo runs each test on its own thread, so the helper `ui::ui_test_init()` uses a `thread_local!` guard (`thread_local!(static INIT: Cell<bool>)`) and calls `i_slint_backend_testing::init_no_event_loop()` once **per thread**, never behind a process-wide `std::sync::Once`. `tests/ui_operator_flows.rs` uses the same thread-local guard. Glue exposes `apply_snapshot(Arc<OperatorSnapshot>)` and `apply_live(LiveUpdate)` so tests can drive it without the event loop. With `init_no_event_loop()` there is no event-loop proxy, so `invoke_from_event_loop` and `spawn_local` both fail; tests use `Ops::inline(app)`, which runs the action on a local current-thread runtime and calls `done` before returning. `Thumbs` and the preview grab worker have the same inline mode.
- **File size:** each `.slint` file and each glue file covers one screen, column or dialog and stays under about 400 lines.
- **Dialogs** are overlays inside the operator window. While one is open, or while a confirm/prompt overlay is open with no dialog under it, F1–F5 still work, Ctrl+B is ignored, and every other operator key is off (`operator.js:306-309`; the web's `confirm()`/`prompt()` block everything).
- **Translatable strings (added November 2, 2026):** every user-visible string literal in the new `.slint` files is wrapped in `@tr("...")` (text unchanged). User-facing strings built in Rust glue live in `src/strings/<area>.rs` modules (one `pub const` or small `fn` per string), following the FEAT specs on `feat-roadmap`. Tests still compare against the exact English text.
- **Bible sources are merged (branch `bible-esv`, November 2, 2026):** `App::available_translations()` returns `Vec<String>` (KJV, WEB, BSB, plus any translation with a saved key or imported file); `settings/translation` accepts `app.translations()`; `state_json` has `bible_sources` and `bible_providers`. Spec: `docs/superpowers/checks/2026-11-bible-sources.md`. The web operator's Bible settings (`web/operator.js`) already show this; Task 11's translation picker and Task 20's Bible tab follow the web, not the older text below.
- **Narrow rule:** at window width ≤ 1250 logical px, hide the brand word, show Screens/Phone as icons only, and show status pills as dots only (`operator.css:239`).

### Product decisions (defaults used; ask Jake only if he wants a change)

1. **No separate Preview step.** As in the web operator, clicking a thumbnail or pressing Enter takes the slide live. The v1 spec's "load into Preview" (§8.1) is not built. The live column monitor is the "Preview" the spec mentions.
2. **Esc inside a text field** clears focus back to the window's key scope (the same effect as the web `blur()`).
3. **The Bible translation** is saved in `Settings.translation` (it exists but is unused today) instead of browser localStorage, so it survives a restart. It is saved through a new `settings/translation` action, so a translation click never rewrites the whole Settings.
4. **Alt+1…Alt+6** inside the Bible box pick a translation (v1 spec §8.2). This is the only new shortcut. It is listed in Help.
5. **Search** in the Add dialog lists still matches any field (JSON substring), as today.
6. **Lines per slide** still offers 1–6, as today.
7. **Help wording:** the web Help rows "→ Space Page Down" and "← Page Up" leave out ↓ and ↑, which the web does bind (`operator.js:312-313`). Native Help says "→ ↓ Space Page Down" and "← ↑ Page Up".
8. **New notes the web lacks, required by the native spec:** Settings → Look says when the chosen font is not installed (§4), and video/audio items say "The video player is missing" when libmpv is missing (§5).
9. **v1 spec features not built** (the web operator lacks them too; this plan keeps it that way): Library tabs in the left column (§12.1), Displays "Identify" (§10), the Preflight button (§13), the stage message box (§12.2), service bundle export (§11), manual verse-break editing and the missing-verse note (§8.2, §8.4), song arrangement drag-to-reorder (§9.1), the "Live item changed — Take Live to update." notice (§7.2; not in `src/live.rs`), and in-app audio device selection with the missing-device notice (native spec §5, v1 §6.4; `Settings` has no audio device today).

## Review Focus

1. **Stale command after a fast double Next.** The operator presses → twice before the first `Live` update arrives. Expected: both send the same `base`, the second is rejected as stale and nothing is shown, and the room advances once. Test in Task 10 (`two_nexts_from_same_base_advance_once`).
2. **F-keys while typing or with a dialog open.** F1 in the Bible box, or F3 inside the song editor's lyrics. Expected: the clear runs, no character is typed, and the dialog stays open. Other keys go to the field. Test in Task 10.
3. **Ctrl+B with a dialog open.** Expected: nothing happens (web behaviour). The Bible box is not focused and the dialog keeps focus. Test in Task 10.
4. **Bible Enter twice.** Enter in the box adds the passage after the live item (or at the end), arms it and selects it. Enter outside the box shows armed slide 0. The arrow/Page/Space keys, clicking a row or clicking a thumbnail clear the arm; the Back/Next **buttons** do not (`operator.js:343-344`). Each Enter does exactly one thing (v1 acceptance row "Navigation"). Test in Task 11 and in the UI test of Task 22.
5. **Cross-chapter range in a one-chapter book** (`Jude 1:1-3:1`). Today `bible.rs:84` loops forever. Expected: the error "Jude 2 does not exist." and the lookup returns. Test in Task 1.
6. **Selected item removed** from the service, by this operator or by another, such as the phone. Expected: the selection falls back to the live item, then the first item, then none (`operator.js:73`), and no panic from an old row index. Test in Task 4.
7. **Reorder while an update arrives.** A drag drop sends `service/move {id, to}` by **item id**. While a drag is in progress, `Data` updates do not rebuild the service model (that would destroy the dragging `DragArea`); the newest one is applied on `drag-finished`. The drop uses the id carried in the drag data, not a row index. Test in Task 6.
8. **Live preview of video with the room closed.** Expected: no second player. The preview shows black with "Room screen is off". The transport still shows the room-reported times, which are absent while closed, so it shows "—". Test in Task 9.
9. **Grabber outlives the player.** A 4 Hz `screenshot-raw` is in flight while the room retires the video. The grabber holds `Weak<Core>` and upgrades it for each grab; while it holds the upgraded `Arc<Core>`, `Core::drop` (which destroys the handle on the `mpv-destroy` thread) cannot run. Expected: no use-after-free, and `grab()` returns `None` once the core is gone (the core lives on while a render context still holds it). Test in Task 8 (gated on `RP_VIDEO_TESTS`).
10. **Grab blocked by a room that does not redraw.** With `vo=libmpv` the screenshot is served on the render thread (the room's Slint render). Expected: the grab uses `mpv_command_async` with a 500 ms timeout and returns `None`, so a minimised room never stalls the grab worker. Test in Task 8; manual in C6.
11. **Many files dropped at once** (winit sends one `DroppedFile` per file). Expected: one batch is processed, and the "Make N pictures into one set of slides?" question is asked once when every file is an image and there is more than one. Test in Task 15 (pure batching). The real drag is a manual check.
12. **A dialog left open while its data disappears** (Item edit open while another client removes the item). Expected: Save shows the core's "That item is gone." toast and the dialog stays open. Test in Task 18.
13. **Unsaved song editor closed with Esc.** Expected: same as the web, where Esc on a `<dialog>` closes it without asking. Test in Task 17 (Esc closes; nothing is saved).
14. **Typed text survives updates.** A phone or a conversion sends a `Data` update while the user types in Item edit, the song editor or Settings. Expected: nothing they typed is lost, because only Add, Screens and Phone refresh (`operator.js:388, 827, 859`). Test in Tasks 13 and 18.

---

### Task 1: Core fixes for an in-process operator

**Files:**
- Modify: `src/app.rs`, `src/bible.rs`, `src/ui/screens.rs` (`NativeScreens` holds one `ScreenGuard` per open native window), `src/live.rs` only if needed for tests

**Interfaces:**
- `pub fn App::available_translations(&self) -> Vec<&'static str>` (today `["KJV"]`), used by `state_json`.
- New action `settings/translation` `{translation: String}` → `{}`. It rejects names not in `bible::TRANSLATIONS` ("Unknown translation."), sets `settings.translation`, and saves through `edit`.
- `backup/now` runs `store.backup` in `tokio::task::spawn_blocking`.
- `display/close` rejects roles other than room, stage and operator with "Unknown screen.".
- `app::Job` fields become `pub(crate)` (today private, `app.rs:118`), so `ui::operator` can read jobs.
- `pub struct ScreenGuard` + `pub fn App::screen_guard(self: &Arc<Self>, role: &str) -> ScreenGuard` (called on an `Arc<App>`: the guard keeps a `Weak<App>` to lower the count on drop). It increments `room_conns` or `stage_conns` and decrements them on drop. `NativeScreens` (Plan 2 Task 8, `src/ui/screens.rs`) takes a guard when a native room or stage window opens and drops it when the window closes, so `room_connected`/`stage_connected` are true for native windows.
- `bible.rs` `lookup`: inside the verse walk, when `last_verse(book, c)` is `None`, return `Err("{Book} {c} does not exist.")`.

- [ ] **Step 1: Write the failing tests**

```rust
// src/bible.rs
#[test]
fn range_past_the_last_chapter_of_a_short_book_is_an_error_not_a_hang() {
    let err = kjv().lookup("Jude 1:1-3:1").unwrap_err();
    assert_eq!(err, "Jude 2 does not exist.");
}

// src/app.rs
#[tokio::test]
async fn settings_translation_saves_and_rejects_unknown() {
    let app = test_app();
    app.do_action("settings/translation", br#"{"translation":"NIV"}"#).await.unwrap();
    assert_eq!(app.data.lock().unwrap().settings.translation, "NIV");
    assert_eq!(app.do_action("settings/translation", br#"{"translation":"XYZ"}"#).await.unwrap_err().message, "Unknown translation.");
}

#[tokio::test]
async fn display_close_rejects_unknown_role() { /* "Unknown screen." */ }

#[test]
fn screen_guard_marks_room_connected_until_dropped() {
    let app = test_app();
    let g = app.screen_guard("room");
    assert_eq!(app.state_json()["room_connected"], true);
    drop(g);
    assert_eq!(app.state_json()["room_connected"], false);
}
```

- [ ] **Step 2:** `cargo test app:: bible::`. The tests fail, and the Jude test hangs, so run it alone with a timeout until the fix is in.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo build` (Tauri) and `cargo test` pass.
- [ ] **Step 5:** Commit `feat(app): translation setting, screen guards, blocking backup off the runtime; fix Bible range hang`.

### Task 2: In-process import

**Files:**
- Modify: `src/app.rs`, `src/server.rs` (upload uses the shared function)

**Interfaces:**
- `pub enum ImportPurpose { Media, Asset, PowerPoint, Lyrics }`. `Lyrics` sends the file to `songselect::import_txt`, exactly like the `song/import-txt` action.
- `pub enum Imported { Media(MediaFile), Asset(AssetId), Job(Uuid), Song { id: Uuid, title: String, duplicate_of: Option<Uuid> } }`
- `impl App { pub async fn import_path(self: &Arc<Self>, path: &Path, purpose: ImportPurpose, progress: impl Fn(u8) + Send + Sync + 'static) -> Result<Imported, ActionError> }`. The file is **copied** into the store's tmp first (never moved: it is the user's file), hashing on a `spawn_blocking` thread and calling `progress(0..=100)` as bytes are copied and hashed. It then calls the shared `App::import_staged(tmp, name, sha, purpose)`: ext sniffing by name, `store.publish`, media kind, `plan_video`/`plan_audio`, `spawn_conversion`, and the PowerPoint job. Error strings are the same as the HTTP path ("RustyPresenter doesn't know how to show .{ext} files.", "Could not stage the presentation.", …).
- `server::upload` keeps streaming to `tmp/*.part` and then calls `import_staged`, so there is one copy of the logic. Move the upload logic, don't copy it.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn import_path_adds_a_picture_to_the_media_library() {
    let app = test_app();
    let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
    image::RgbaImage::new(4, 4).save(&png).unwrap();
    let Imported::Media(m) = app.import_path(&png, ImportPurpose::Media, |_| {}).await.unwrap() else { panic!() };
    assert_eq!((m.kind, m.status.clone()), (MediaKind::Image, MediaStatus::Ready));
    assert!(png.exists(), "the user's file is copied, never moved");
    assert_eq!(app.data.lock().unwrap().media.len(), 1);
}

#[tokio::test]
async fn import_path_rejects_unknown_extensions_with_the_web_message() {
    let p = std::env::temp_dir().join(format!("{}.xyz", Uuid::new_v4()));
    std::fs::write(&p, b"x").unwrap();
    let e = test_app().import_path(&p, ImportPurpose::Media, |_| {}).await.unwrap_err();
    assert_eq!(e.message, "RustyPresenter doesn't know how to show .xyz files.");
}

#[tokio::test]
async fn import_path_lyrics_imports_a_songselect_song() { /* fixture from songselect tests; Imported::Song with title */ }

#[tokio::test]
async fn import_path_reports_progress_up_to_100() { /* collect progress values; last is 100 */ }
```

- [ ] **Step 2:** `cargo test app::` → FAIL.
- [ ] **Step 3:** Implement. The HTTP upload tests (if any) behave as before.
- [ ] **Step 4:** `cargo build` and `cargo test` pass. Upload a video and a `.pptx` through the Tauri web operator: works as before.
- [ ] **Step 5:** Commit `feat(app): in-process import shared with HTTP upload`.

### Task 3: Theme global, icons and shared widgets

**Files:**
- Create: `ui/icons.slint`, `ui/widgets.slint`, `ui/operator.slint` (placeholder only: `export component OperatorWindow inherits Window {}`; Task 5 fills it in)
- Modify: `ui/theme.slint` (created by Plan 2 Task 5; extend it), `ui/app.slint` (export `Theme`, `OperatorState` and `OperatorWindow` for Rust), `src/ui/mod.rs`

**Interfaces:**
- `ui/theme.slint`: `export global Theme { out property <color> bg: #0d0f13; panel: #15181e; panel-2: #1b1f27; raise: #232833; line: #2a303b; text: #eceef3; muted: #8d95a8; faint: #5d6576; accent: #f5b94a; accent-ink: #1a1405; live: #ff4f55; ok: #3ccf91; … }`. Every hard-coded colour in `web/operator.css` gets a named token: `dash` #3a4150, `btn-hover` #3c4352, `thumb-hover` #59627a, `nav-hover` #444c5c, `clear-hover` #4a5262, `danger-text` #ff8f93, `notice-text` #ffc1c3, `notice-bg` #ff4f551f, `notice-border` #ff4f5566, `clear-all-bg` #2a1517, `clear-all-border` #ff4f5573, `clear-all-text` #ffb0b3, `monitor-outline` #ff4f558c, `upnext-text` #f2d08f, `focus-ring` #f5b94a24, `add-hover` #f5b94a14, `kbd-bg` #ffffff0f, `play-bg` #0000008c, `scroll-thumb` #2c323d, `toast-bg` #2a2f3a, `toast-err-bg` #3a1b1d, `toast-err-border` #6b2b2f, `toast-err-text` #ffd3d5, `drop-scrim` #0d0f13d1, `dialog-scrim` #050608b3. Also `radius: 12px`, `font: "Segoe UI Variable Text"`, `display: "Segoe UI Variable Display"` (falls back to Noto Sans where missing, as in Plan 1), `base-size: 14px`. All `Theme` properties are `out` constants; a component cannot bind a global's property, so `Theme` holds no state.
- `ui/theme.slint` also exports `global OperatorState { in-out property <bool> focus-in-text; }`. Each text field in the operator and dialogs sets it in its `changed has-focus` handler (true on focus, false on blur), and the key scope reads it (Task 10).
- Narrow: `OperatorWindow` has `out property <bool> narrow`, set to `root.width <= 1250px` in both `init` and `changed width` (a plain binding is a binding loop: the top bar's width needs depend on `narrow`, and the window's width on those; `changed` alone misses a window whose first width never changes), and passes it down as an `in property <bool> narrow` to the top bar and pills (Task 5). It is not a `Theme` property.
- `ui/icons.slint`: `export component Icon inherits Path { in property <string> name; … }` with the 24×24 stroke paths from `operator.html:13-23` and `operator.js:20-38` (song, scripture, video, image, audio, deck, text, edit, trash, play, pause, close, plus, upload, restart, dots, phone, copy, chevron-down, chevron-left, chevron-right, monitor, gear, book). Defaults: 18×18, stroke `currentColor` → an `in property <brush> color`, stroke-width 2, no fill. `play` and `pause` take `filled: true`.
- `ui/widgets.slint`: `Btn` (`primary`, `danger`, `small`, `enabled`, `tooltip`; `clicked`), `TopBtn` (icon + label, `icon-only` for Settings, `narrow` for Screens/Phone in a narrow window — label hidden, 8×10 padding kept as `operator.css:180`, `tooltip`), `Pill` (`label`, `state: "off" | "on" | "warn"`, tooltip text), `Kbd` (`text`), `Chip` (`text`, `state: "ok" | "warn" | "bad" | ""`, `tooltip`), `Segmented` (`[string]` labels, `in-out property <int> index`, `changed(int)`), `Panel` (+ `PanelHead` with title and count), `Bar` (progress 0–100, `height`, width animated 500 ms linear unless `animated: false` — the job bar, whose `.job .bar i` has no transition), `Field` (label + child slot), `ScrollBox` (styled scrollbar: 10 px, radius 6), `CopyText` (a read-only `LineEdit` in `Consolas, monospace` on `Theme.bg` with a 1 px `line` border and radius 6, so the text can be selected and copied like the web `.copy` with `user-select: all`, `operator.css:194`). Tooltips use Slint's built-in `Tooltip` element with `TipBox` content (`ui/tip.slint`), placed as `if root.tooltip != "": Tooltip { … }`: it is an input-transparent tooltip popup, so a button rested on until its tip shows still takes the click (a plain `PopupWindow` would swallow it). `CopyText` is not a text field for the keys (the web `.copy` is a `<span>`) and does not touch `OperatorState`. All sizes as in `operator.css` (`.btn` 8×13 padding, radius 9, weight 600; `.small` 5×10, 13 px; disabled opacity .45; …).

- [ ] **Step 1: Write the failing test** `src/ui/mod.rs`:

```rust
#[test]
fn theme_matches_operator_css() {
    ui_test_init(); // thread_local-guarded init_no_event_loop()
    let w = crate::ui::OperatorWindow::new().unwrap(); // placeholder window from this task: Theme only needs a component
    let t = w.global::<crate::ui::Theme>();
    assert_eq!(t.get_bg(), slint::Color::from_rgb_u8(0x0d, 0x0f, 0x13));
    assert_eq!(t.get_accent(), slint::Color::from_rgb_u8(0xf5, 0xb9, 0x4a));
    assert_eq!(t.get_live(), slint::Color::from_rgb_u8(0xff, 0x4f, 0x55));
}
```

- [ ] **Step 2:** `cargo test ui::` → FAIL.
- [ ] **Step 3:** Implement the global, icons and widgets. Add a debug-only `examples/widgets.rs` that shows every widget in each state, for a person to compare against the web page. (As built: the gallery window `WidgetGallery` lives in `ui/gallery.slint`, exported from `ui/app.slint` so the example and a test can use it; its strings are plain literals, not `@tr`, so they stay out of the translation catalog.)
- [ ] **Step 4:** `cargo test` passes. **Manual:** `cargo run --example widgets` beside the web operator; the colours, radii and font weights match.
- [ ] **Step 5:** Commit `feat(ui): operator theme global, icons and shared widgets`.

### Task 4: Operator view model (pure) and the action client

**Files:**
- Create: `src/ui/operator/mod.rs` (module root only in this task), `src/ui/operator/view.rs`, `src/ui/ops.rs`
- Modify: `src/ui/mod.rs`

**Interfaces:**
- `#[derive(Clone, Debug)] pub struct ItemRow { pub id: Uuid, pub kind: String, pub title: String, pub subtitle: String, pub slides: Vec<Slide>, pub item: ServiceItem }` (no `PartialEq`: `ServiceItem` and `ItemKind` do not derive it, `model.rs:118,130`; tests compare fields).
- `pub struct OperatorSnapshot { pub service_id: Option<Uuid>, pub service_name: String, pub items: Vec<ItemRow>, pub translations: Vec<String>, pub available: Vec<String>, pub translation: String, pub bible_sources: Vec<BibleSource>, pub bible_providers: Vec<BibleProvider>, pub room: RoomStatus, pub stage_connected: bool, pub stage_configured: bool, pub phones: usize, pub jobs: Vec<JobRow>, pub settings: Settings, pub data: Arc<Data>, pub monitors: Vec<Monitor>, pub windows: Vec<String>, pub lan_url: Option<String>, pub ffmpeg: bool, pub player_missing: bool, pub backups: Vec<BackupInfo>, pub phones_list: Vec<(Uuid, String)>, pub data_folder: String, pub version: String }`. It is `Send`, built by `OperatorSnapshot::read(app: &App)` from the same sources as `state_json` (no JSON round trip). `stage_configured` is `settings.displays.stage.is_some()`. `pub enum RoomStatus { Showing, Starting, Off }`.
  - **`read` never runs on the Slint thread.** It calls `store.backups()` (parses every backup's `data.json`, `store.rs:107`), `media::ffmpeg()` (a PATH search) and `Screens::monitors()`, which `NativeScreens` serves on the Slint thread with a blocking oneshot, so a read there would deadlock. Only the follower thread (Task 5) calls it.
- `pub fn stage_pill(snap) -> Option<(bool /*on*/)>`: shown when `stage_configured || stage_connected` (`operator.js:105`), `on = stage_connected`.
- `pub fn phones_label(n: usize) -> Option<String>`: `None` for 0, "1 phone", "N phones" (`operator.js:106`).
- `pub fn subtitle(row: &ItemRow, media: &[MediaFile], player_missing: bool) -> String` follows `operator.js:111-121`: "Video · converting 40%", "Video · may not play — needs converter", "Video · conversion failed", "Video · loops", "Song · 4 slides", "Scripture · 1 slide", and kind names Song, Scripture, Video, Picture, Audio, Slides, Announcement. When `player_missing` and the item is video or audio, it is "Video · The video player is missing — reinstall RustyPresenter" (or "Audio · …"), per native spec §5.
- `pub fn reselect(selected: Option<Uuid>, items: &[ItemRow], live_item: Option<Uuid>) -> Option<Uuid>` follows `operator.js:73`.
- `pub fn insert_position(items: &[ItemRow], live_item: Option<Uuid>) -> Option<usize>` returns the index after the live item, else `None` (append), following `operator.js:337-340`.
- `pub fn live_what(view: &LiveView) -> String` returns "Logo", "`title · label`", "♪ title" or "Clear".
- `pub fn next_up(view: &LiveView) -> (String /*small*/, String /*body*/)` returns the title · label plus the text, or "Slide picture", "Video", "Picture" or "Audio". With no next slide it returns "End of service" when something is live, else "—".
- `pub fn live_index(view: &LiveView, selected: Option<Uuid>) -> Option<usize>` follows `markLive`: the cursor slide when the cursor is on the selected item, `view.item` is set and the frame shows something.
- `pub struct TransportRow { pub kind: TransportKind, pub inst: u64, pub label: String, pub paused: bool, pub percent: f32, pub time: String }` and `pub fn transport(view: &LiveView) -> Vec<TransportRow>`. Positions are used only when the status `inst` matches the frame's instance. `time` is "m:ss / −m:ss", or "—" when there is no duration.
- `pub fn fmt_time(s: f64) -> String` returns `m:ss`, clamped at 0 and floored.
- `pub fn count_label(n: usize) -> String` returns "1 item", "N items", or "" when there are none.
- `ui/ops.rs`: `pub struct Ops { app: Arc<App>, rt: Option<tokio::runtime::Handle> }`
  - `Ops::new(app, rt)` and `Ops::inline(app)` (tests: run on a local current-thread runtime and call `done` before returning).
  - `pub fn call<T: DeserializeOwned + Send + 'static>(&self, action: &'static str, body: serde_json::Value, done: impl FnOnce(Result<T, String>) + 'static)`: `slint::spawn_local(async move { let r = rt.spawn(do_action).await; done(r) })`. The core call runs on tokio; `done` runs on the UI thread and has no `Send` bound.
  - `pub fn cmd(&self, cmd: Command, base: Option<u64>, done: impl FnOnce(Outcome) + 'static)` wraps `App::command` with a fresh `Uuid` id, same pattern.
  - `pub fn import(&self, path: PathBuf, purpose: ImportPurpose, progress: impl Fn(u8) + Send + Sync + 'static, done: impl FnOnce(Result<Imported, String>) + 'static)`.
  - `pub fn outcome_toast(o: &Outcome) -> Option<(String, bool /*err*/)>` implements the toast rule from the Global Constraints.

- [ ] **Step 1: Write the failing tests** in `view.rs`: `subtitle_for_each_kind_and_media_state`, `subtitle_pluralises_slides`, `subtitle_says_player_missing_for_video_and_audio`, `stage_pill_shown_when_configured_or_connected`, `phones_label_singular_plural_and_hidden`, `reselect_keeps_existing_selection`, `reselect_falls_back_to_live_then_first_then_none`, `insert_position_after_live_item_or_append`, `live_what_priority_logo_slide_audio_clear`, `next_up_text_placeholders_and_end_of_service`, `live_index_only_when_cursor_on_selected_and_on_air`, `transport_ignores_status_from_other_instance`, `transport_time_format_and_dash_without_duration`, `fmt_time_clamps_and_floors`, `count_label_empty_for_zero`. In `ops.rs`: `outcome_toast_silent_for_stale`, `outcome_toast_plain_for_end_of_service`, `outcome_toast_error_otherwise`, `inline_call_returns_core_error_message` (`song/save` with a blank title → `Err("Give the song a title.")`), `inline_done_may_capture_rc` (a `done` closure holding an `Rc<RefCell<_>>` compiles and runs).
- [ ] **Step 2:** `cargo test ui::operator::view ui::ops` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): operator view model and in-process action client`.

### Task 5: Operator window shell, top bar and update follower

**Files:**
- Create: `ui/operator/topbar.slint`, `src/ui/operator/window.rs`
- Modify: `ui/operator.slint` (placeholder from Task 3), `src/ui/operator/mod.rs`

**Interfaces:**
- `ui/operator.slint`: `export component OperatorWindow inherits Window`. Title "RustyPresenter", min size 1024×640, background `Theme.bg`, default font `Theme.font` 14 px. The 60 px top bar sits over a 3-column body: `min 250 / max 300 | stretch | min 360 / max 440`, with 10 px gap and padding. The column bodies are placeholder `Panel`s that Tasks 6–9 fill. `out property <bool> narrow`, set to `root.width <= 1250px` in `init` and in `changed width` (not a binding; see Global Constraints), passed down.
- Top bar properties: `in property <bool> narrow`, `in property <string> service-name`, `in property <[TranslationChip]> translations` (`{name, on, off}`), `in-out property <string> bible-text`, `in property <bool> room-on`, `in property <string> room-state` ("on"/"warn"), `in property <string> room-tip`, `in property <bool> stage-shown`, `in property <bool> stage-on`, `in property <string> phones-label` (empty hides the pill). In a narrow window Screens and Phone pass `narrow: true` to `TopBtn`, not `icon-only` (which would shrink their side padding to 8 px). Callbacks: `services()`, `screens()`, `phone()`, `settings()`, `bible-submit(string)`, `pick-translation(int)`. Element ids: `service-pick`, `bible-input`, `screens-btn`, `phone-btn`, `settings-btn`, `status-room`, `status-stage`, `status-phones`.
- Tooltips use the exact web strings: service picker "Switch or create services" (`operator.html:13`), Settings button "Settings" (`operator.html:23`), pills "Room screen is showing", "Room screen is starting…", "Room screen is not open — click Screens", "Stage display", "Phone remotes connected" (`operator.js:104-106`).
- `src/ui/operator/window.rs`:
  - `pub trait OperatorScreens { fn video_info(&self) -> VideoInfo /* draw mode, mpv versions; for About */; }`, with a blanket `impl<T: OperatorScreens + ?Sized> OperatorScreens for Arc<T>`. `preview_grabber` is added in Task 9, since `FrameGrabber` does not exist until Task 8. `NativeScreens` implements it; tests pass a fake, so no room window or mpv is needed.
  - `pub struct Operator { ui: OperatorWindow, app: Arc<App>, ops: Ops, screens: Rc<dyn OperatorScreens>, state: RefCell<OpState>, … }` where `OpState { snap: Option<Arc<OperatorSnapshot>>, live: Option<LiveUpdate>, selected: Option<Uuid>, armed: Option<Uuid>, dialog: Option<DialogKind>, asks: usize, dragging: bool, held_snapshot: Option<Arc<OperatorSnapshot>> }`.
  - `Operator::open(app: Arc<App>, ops: Ops, screens: Rc<dyn OperatorScreens>) -> Result<Rc<Operator>, String>`
  - `pub fn apply_snapshot(&self, s: Arc<OperatorSnapshot>)`, `pub fn apply_live(&self, v: LiveUpdate)`, `pub fn window(&self) -> &slint::Window`, `pub fn raise(&self)` (show, un-minimise, focus), `pub fn request_refresh(&self)` (asks the follower for a fresh full read, monitors included).
  - `follow(app, rx, weak)`: a plain thread like `ui::stage::follow`. On `Update::Data` it builds `OperatorSnapshot::read(&app)` on that thread (coalescing like `refreshSoon`: only the newest pending snapshot is applied). On `Update::Live` it keeps only the newest. `Update::Job{id,percent}` and `Update::Convert{id,percent}` patch the current snapshot. On `Lagged` and on `request_refresh` it re-reads both. **`OperatorSnapshot::read` always sets `player_missing: false`** (the core cannot tell whether libmpv loaded): every full read in the follower must be followed by `.with_player_missing(..)` with the answer from the screens/video layer, and every `Convert` patch to `data.media` must call `refresh_subtitles()`, or the "The video player is missing" subtitle (native spec §5, Product decision 8) never shows. Add a test that a snapshot applied with the player missing shows that subtitle on a video row. The first full read at open also happens on this thread (the web `onopen`); until it arrives the window shows empty columns, the service name "Service", and no Welcome panel.

- [ ] **Step 1: Write the failing tests** (`src/ui/operator/window.rs`, testing backend, fake `OperatorScreens`): `top_bar_shows_service_name_or_service`, `room_pill_warns_when_room_not_connected` (`status-room` state "warn" and tooltip "…not open — click Screens"), `room_pill_on_with_screen_guard` (hold `app.screen_guard("room")`, apply a snapshot → "on"), `stage_pill_hidden_unless_configured_or_connected` (configured but not connected → shown, off), `phones_pill_label` ("1 phone", "2 phones", hidden at 0), `narrow_hides_brand_word` (set the window size to 1200×800 → `narrow` is true), `narrow_matches_the_first_width` (a window that keeps its first, narrow width is narrow), `before_first_snapshot_shows_no_welcome`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): operator window shell, top bar and update follower`.

### Task 6: Service column

**Files:**
- Create: `ui/operator/service.slint`, `src/ui/operator/service.rs`

**Interfaces:**
- `export struct ServiceRow { id: string, num: int, kind: string, title: string, subtitle: string, selected: bool, live: bool }`
- `ServiceColumn` component: `in property <[ServiceRow]> rows`, `in property <string> count`. It has a `ListView` with one delegate per row, sized as in `operator.css:60-75` (gap 10, padding 9×10, radius 10, kind chip 30×30, 4 px live bar at left −8). Edit (tooltip "Edit") and trash (tooltip "Remove from service") appear on hover (`operator.js:147-148`). There is an empty state ("Your service is empty" + "Click “Add to service” below, type a Bible verse up top, or drag files onto this window.") and the dashed **Add to service** button.
  - Callbacks: `select(string)`, `show-first(string)` (double-click), `edit(string)`, `remove(string)`, `add()`, `move(string /*id*/, int /*to*/)`, `drag-started()`, `drag-finished()`.
  - Element ids: `service-list`, `add-btn`, `row-<n>` via `accessible-id`.
- Drag to reorder: `DragArea { allow-move: true; data: root.drag-data(row.id); }` puts plain text `svc:<id>` in the data. Each row's `DropArea` accepts only `svc:` data from a different id. Dropping on row `i` sends `service/move {id, to: i}`, where `i` is the index of the row dropped on, with no top/bottom-half logic; dropping a row on itself does nothing (`operator.js:139-141`). Hovering shows the row's 2 px accent top border (`operator.css:74`), and the dragged row gets opacity .4.
- Rust: `pub fn rows(snap: &OperatorSnapshot, selected: Option<Uuid>, live_item: Option<Uuid>) -> Vec<ServiceRow>`. The service model is one `Rc<VecModel<ServiceRow>>` that is updated with `set_vec` on Data and `set_row_data` on Live/selection changes, never replaced. **While `dragging` is true, `apply_snapshot` stores the snapshot in `held_snapshot` and does not rebuild the model** (a rebuild destroys the dragging delegate); `drag-finished` applies the held snapshot.
- Behaviour: click selects and clears the arm (preview only). Double-click runs `cmd(Show{item, 0})` if the item has slides. Trash runs `service/remove {id}` with no confirm. Edit opens Item edit (Task 18). Add opens the Add dialog on the Song tab (Task 16).

- [ ] **Step 1: Write the failing tests:** `rows_mark_selected_and_live`, `drop_on_row_sends_its_index` (drag 0 → drop on row 2 sends `to: 2`; drag 3 → drop on row 0 sends `to: 0`; drop on itself → no call), `click_selects_without_going_live` (live version unchanged), `double_click_shows_slide_zero`, `trash_removes_without_confirm`, `data_update_mid_drag_is_held_until_drag_finished`, `drop_uses_dragged_id_after_rows_rebuilt` (start a drag on row 1, apply a snapshot that inserts a row at 0, drop → `service/move` moves the original id), `empty_state_text`.
- [ ] **Step 2:** FAIL → **Step 3:** implement. Use `ElementHandle::mock_drag` where it works. Otherwise call the `move` callback directly.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): operator service column with drag to reorder`.

### Task 7: Slides grid and thumbnails

**Files:**
- Create: `ui/operator/slides.slint`, `src/ui/operator/slides.rs`, `src/ui/thumbs.rs`

**Interfaces:**
- `export struct SlideCell { idx: int, label: string, image: image, live: bool, play: bool }` and `export struct SlideRowCells { cells: [SlideCell] }`.
- `SlidesPanel`: a header (kind icon in accent, 19 px display title, **Edit** `Btn.small`), then a `ListView` of rows. Cells are 16:9 screens with radius 9 and a 2 px line outline (3 px live outline, hover `thumb-hover`), a LIVE badge, a play overlay (54 px circle, filled play icon) and a caption.
  - Header title (`operator.js:162`): the selected item's title; with nothing selected, "Pick an item on the left" when the service has items and "Welcome" when it is empty. The **Edit** button shows only when an item is selected (`operator.js:164-166`).
  - Empty states: "Choose something from the service list.", "Nothing to show" + "This item has no slides yet. Click Edit to add some.", and the Welcome panel with the 4 steps from `operator.js:199-209`.
  - `in property <length> content-y-target`, `out property <length> grid-width`.
  - Callbacks: `show(int)`, `edit()`, `columns-changed(int)`.
  - Element ids: `slides-title`, `slides-edit`, `slide-<idx>` via `accessible-id`.
- `pub fn columns(width_px: f32) -> usize` follows the CSS auto-fill rule: `max(1, floor((w - 32 + 14) / (230 + 14)))`.
- `pub fn chunk_rows(n: usize, cols: usize) -> Vec<Vec<usize>>` and `pub fn row_of(idx: usize, cols: usize) -> usize`, for scroll-into-view. The live row is scrolled into view (nearest) by setting `content-y` whenever the live index changes. Unlike the web, it does not scroll again on every live update (the web's `markLive` never records what it has already scrolled to).
- `src/ui/thumbs.rs`: `pub struct Thumbs { renderer: Arc<Renderer>, store_root: PathBuf, cache: … }` with `pub fn request(&self, key: ThumbKey, job: ThumbJob, done: impl FnOnce(SharedPixelBuffer<Rgba8Pixel>) + 'static)`. The work runs on a worker; `done` runs on the UI thread through `slint::spawn_local` awaiting a oneshot (no `Send` bound). An inline mode runs synchronously for tests.
  - **Own renderer:** `Thumbs` uses its own `Renderer::with_cap(..)` instance, never the room's. `Renderer` is one worker with one FIFO (`render/cache.rs:51-80`), so sharing it would queue a whole service's thumbnails ahead of the room's next slide.
  - `#[derive(Clone, PartialEq, Eq, Hash)] ThumbKey { render: Option<RenderKey>, layers: String, size: (u32, u32) }`. `RenderKey` is already `Eq + Hash` (`render/cache.rs:10`); `layers` is the serde JSON of the background and media layer, because `Slide` and `Background` are not `Eq`/`Hash` (`model.rs:8,46`).
  - **Thumbnails never draw the logo** (the web passes `logo: false`, `operator.js:183`), so the key has no logo field.
  - Text slides: background (colour, or image decoded at size) + `Renderer::render(RenderKey{…, canvas: size})`, composited on a worker.
  - Image/still: decoded at size with `image`.
  - **Video in this plan comes out black:** video slides, text slides over a video background (`render.js:36, 120, 122, 135-137`) and video background choices (Task 18) show black plus, for video/audio slides, the play overlay drawn by Slint. Video frames come in Plan 4 with the mpv probe.
  - The background is `item.background` or `settings.background`, as in `itemBackground`.
  - Size = cell width × the window scale factor, rounded up to 64 px steps (16:9), so resizing does not re-render each frame. The placeholder is an empty `image`, which the cell draws as black.
- Behaviour: clicking a thumbnail clears the arm and runs `cmd(Show{item, idx})` (takes it live straight away). Edit opens Item edit.

- [ ] **Step 1: Write the failing tests:** `columns_follow_css_auto_fill` (558 px → 2, 230 → 1, 1000 → 4), `chunk_rows_and_row_of`, `thumb_size_rounds_to_64px_steps`, `thumb_key_ignores_logo`, `click_thumb_takes_slide_live` (the live cursor equals `(item, 2)`), `live_badge_only_on_cursor_slide_of_selected_item`, `welcome_shown_when_service_empty`, `header_title_pick_an_item_or_welcome` (items but none selected → "Pick an item on the left"; empty → "Welcome"), `edit_button_only_with_selection`, `thumb_text_slide_renders_non_blank` (a worker render of "Amazing grace" at 320×180 has at least one non-background pixel).
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): slides grid with compositor thumbnails`.

### Task 8: Video frame grabber

**Files:**
- Modify: `src/video/ffi.rs` (`mpv_command_async`, `mpv_free_node_contents`, `mpv_node`, `mpv_node_list`, `mpv_byte_array`, `MPV_FORMAT_NODE_MAP`/`BYTE_ARRAY`, `MPV_EVENT_COMMAND_REPLY`), `src/video/player.rs` (`FrameGrabber`)

**Interfaces:**
- `video::player`: `#[derive(Clone)] pub struct FrameGrabber { core: Weak<Core> }` (`Core` is the private ref-counted owner of the mpv handle, `player.rs:61-79`).
  - `pub fn Player::grabber(&self) -> FrameGrabber`.
  - `pub fn FrameGrabber::grab(&self, max_w: u32) -> Option<SharedPixelBuffer<Rgb8Pixel>>` upgrades the `Weak`; while it holds the upgraded `Arc<Core>`, `Core::drop` cannot destroy the handle. It sends `screenshot-raw video` with `mpv_command_async` and waits for the matching `MPV_EVENT_COMMAND_REPLY` (routed by reply id from the player's event thread) for at most **500 ms**; on timeout or error it returns `None`. On success it reads the node map `{w, h, stride, format: "bgr0", data}`, converts BGRx → RGB, downscales to `max_w` (Triangle) and frees the node. It returns `None` once the core is gone.
  - Why async: with `vo=libmpv` the screenshot is served on the render thread, which is the room's Slint render, so a synchronous `mpv_command_ret` would wait until the room redraws. A minimised or idle room would then stall both the grab worker and playback.

- [ ] **Step 1: Write the failing tests** (gated `RP_VIDEO_TESTS=1`):
  - `grabber_returns_frame_of_playing_testsrc`: a 320×240 lavfi `testsrc2` player with `vo=libmpv` and a software render context. The test drives `mpv_render_context_render` on a helper thread while grabbing (or uses `vo=null` if C6 shows that works); the grab is non-uniform.
  - `grab_without_render_times_out_with_none`: the same player with nobody rendering; `grab` returns `None` within about 600 ms.
  - `grab_returns_none_once_the_core_is_gone`: drop the player and its render context on another thread while grabbing in a loop for 1 s: no crash, and once the last `Arc<Core>` is gone the result is `None`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes. With `RP_VIDEO_TESTS=1 RP_LIBMPV=…` the gated tests pass on Windows.
- [ ] **Step 5:** Commit `feat(video): frame grabber for the live preview`.

### Task 9: Live column and live preview

**Files:**
- Create: `ui/operator/live.slint`, `src/ui/operator/live.rs`, `src/ui/preview.rs`
- Modify: `src/ui/room.rs` and `src/ui/screens.rs` (expose the drawn player's grabber; `NativeScreens` implements `OperatorScreens`)

**Interfaces:**
- Add `fn preview_grabber(&self) -> Option<FrameGrabber>` to the `OperatorScreens` trait (`src/ui/operator/window.rs`), to its `Arc<T>` blanket impl, and to the tests' `FakeScreens` (returning `None`); Task 5 left it out because `FrameGrabber` did not exist yet.
- `NativeScreens::preview_grabber(&self) -> Option<FrameGrabber>` returns the grabber of the player `VideoSurface` is drawing (foreground video, else background video), or `None` when the room is closed. Add `Room::drawn_grabber()` for it.
- `src/ui/preview.rs`: `pub struct Preview` prepares the live frame at preview size off the UI thread with the same layer rules as the room: background colour/image, still, slide via `Renderer`, logo, and `room::draw_background(frame)`. `LiveMonitor` has the same inputs as `RoomWindow` (`ui/room.slint:54-65`): `Layer` structs `bg`, `media`, `slide`, `logo`, plus `bg-visible`, `video`, `video-frame` and `black`, so the two screens stay in step.
  - While the frame has a video layer, a `slint::Timer` at 250 ms sends `grabber.grab(preview_px)` to a worker and sets `video-frame` through `spawn_local` on return (no `Send` bound on the UI side). It skips a tick while a grab is still running.
  - With no grabber (room closed): `video-frame` is empty and the caption reads "Room screen is off".
- `LiveColumn` component, top to bottom (sizes from `operator.css:109-142`):
  - The LIVE row: dot, "LIVE", "updating…" while `pending`, and `live-what` on the right.
  - `LiveMonitor`: 16:9 with a `monitor-outline` outline.
  - `Transport`: one row per `TransportRow`, with a 34 px play/pause button (tooltip "Play" while paused, else "Pause", `operator.js:272`), label, `Bar` and time.
  - Notice box.
  - Back/Next grid (1 : 1.6, 64 px tall; Next uses accent). Tooltips: "Back (← or Page Up)" and "Next (→, Space or Page Down)".
  - Clear grid: **Clear All F1** across the full width, then Text F2, Media F3, Audio F4, Logo F5. Logo shows `.on` while `frame.logo` is set.
  - Up next box (min 70, max 110, clipped).
  - Element ids: `next-btn`, `prev-btn`, `clear-all`, `clear-text`, `clear-media`, `clear-audio`, `clear-logo`, `live-what`, `pending`, `notice`, `up-next`, `transport-video`, `transport-audio`.
- Behaviour: Back/Next send `prev`/`next` with `base` and **leave the arm alone** (`operator.js:343-344`; only the keys clear it). Clears send no base. Play/pause sends `media_pause` or `audio_pause` `{inst, paused: !paused}`.

- [ ] **Step 1: Write the failing tests**
  - Pure: `transport_rows_from_view` (reuses `view::transport`) and `preview_hides_background_under_foreground_video`.
  - Testing backend (fake `OperatorScreens`): `next_button_advances_live`, `next_button_keeps_the_arm`, `prev_button_goes_back`, `clear_all_button_clears_frame`, `logo_button_marked_on_while_logo_shown`, `pending_marker_while_requested_not_committed`, `notice_shown_from_live_view`, `room_monitor_gone_shows_notice` (feed `Gone(room)` through the Plan 2 `Watcher` → `NativeScreens` handler with a fake monitor list, apply the resulting live update → `notice` shows the room-monitor message; v1 §10, native spec §7), `up_next_end_of_service`, `transport_button_tooltip_play_or_pause`, `video_preview_without_room_says_room_screen_is_off`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Manual check (Windows)**: play a foreground video in the room on the one monitor. The preview updates about 4 times a second, the room's frame rate stays at the source rate (`video_surface --measure` method), and CPU rises by no more than about 5 %. Minimise the room window while the preview grabs: playback keeps going and the preview stops updating without freezing the operator. Record this in `2026-native-checks.md` as row "C6 live preview grab". Also record whether `screenshot-raw` works with `vo=null` (needed by Plan 4 thumbnails).
- [ ] **Step 6:** Commit `feat(ui): operator live column with 4 Hz video preview`.

### Task 10: Keyboard

**Files:**
- Create: `src/ui/operator/keys.rs`
- Modify: `ui/operator.slint` (root `FocusScope keys`), `src/ui/operator/window.rs`

**Interfaces:**
- Key names: Slint delivers special keys as private-use characters (`Key.RightArrow`, `Key.F1`, `Key.PageDown`, …), not web names. `pub fn key_name(text: &str) -> Option<&'static str>` maps the `slint::platform::Key` characters to "ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp", "PageDown", "PageUp", "Enter" (Return), "Escape", "F1"–"F5", and " " for Space; other text passes through as typed (`"b"`, `"1"`).
- Pure routing: `pub enum KeyAction { Clear(Command), FocusBible, Next, Prev, TakeArmedOrSelected, Blur, PickTranslation(usize), None }`, `pub struct KeyCtx { pub in_text_field: bool, pub in_bible_box: bool, pub dialog_open: bool }` and `pub fn route(key: &str, ctrl_or_cmd: bool, alt: bool, ctx: KeyCtx) -> KeyAction`. `dialog_open` is true while a dialog **or any confirm/prompt overlay** is up (`asks > 0`), so a drop question or "Name these slides" with no dialog under it still blocks → and friends, like the web's blocking `confirm()`/`prompt()`. The order is `operator.js:305-320`:
  1. F1–F5 → clear_all / clear_slide / clear_media / clear_audio / logo, always.
  2. Ctrl/Cmd+B → `FocusBible` unless a dialog is open (then `None`).
  3. A dialog open → `None`.
  4. Alt+1…6 in the Bible box → `PickTranslation`.
  5. Esc in a text field → `Blur`.
  6. Any other key in a text field → `None`.
  7. → ↓ PageDown Space → `Next`.
  8. ← ↑ PageUp → `Prev`.
  9. Enter → `TakeArmedOrSelected`.
- Slint: the root `FocusScope` uses `capture-key-pressed` for F1–F5, Ctrl+B, and Alt+1…6 while the Bible box has focus, so they work inside `LineEdit`/`TextEdit` and dialogs and are `accept`ed (in bubbling `key-pressed` the focused `LineEdit` would see the digit first). It uses `key-pressed` (bubbling) for everything else, so a focused field keeps its keys. One callback, `key(string, bool ctrl, bool alt, bool in-text, bool in-bible) -> bool`, sends each key to Rust (`key_name` then `route`). The text-field flags come from `has-focus` on the Bible input and from `OperatorState.focus-in-text` (Task 3). After a dialog closes or a button is clicked, focus returns to `keys` (`keys.focus()`). Closing a dialog also sets `OperatorState.focus-in-text = false`: a dialog is an `if` overlay, and a field destroyed while focused never runs its `changed has-focus` handler, so the flag would otherwise stay set and block the typing keys.
- `FocusBible` focuses the Bible box **and selects all its text** (`operator.js:308`, `select-all()` on the `bible-input` `TextInput`).
- `TakeArmedOrSelected`: use `armed ?? selected`, clear the arm, and if the item has slides run `cmd(Show{id, 0})`. The `Next`/`Prev` keys clear the arm.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn f_keys_clear_even_while_typing_or_in_a_dialog() {
    let ctx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: true };
    assert_eq!(route("F1", false, false, ctx), KeyAction::Clear(Command::ClearAll));
    assert_eq!(route("F5", false, false, ctx), KeyAction::Clear(Command::Logo));
}

#[test]
fn ctrl_b_ignored_while_a_dialog_is_open() {
    assert_eq!(route("b", true, false, KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: true }), KeyAction::None);
}

#[test]
fn arrows_go_to_the_text_field_while_typing() {
    assert_eq!(route("ArrowRight", false, false, KeyCtx { in_text_field: true, in_bible_box: false, dialog_open: false }), KeyAction::None);
}

#[test]
fn slint_key_characters_map_to_route_names() {
    use slint::platform::Key;
    assert_eq!(key_name(&String::from(char::from(Key::RightArrow))), Some("ArrowRight"));
    assert_eq!(key_name(&String::from(char::from(Key::F1))), Some("F1"));
    assert_eq!(key_name(&String::from(char::from(Key::PageDown))), Some("PageDown"));
    assert_eq!(key_name(&String::from(char::from(Key::Return))), Some("Enter"));
}
```

  Also `space_pagedown_down_right_are_next`, `pageup_up_left_are_prev`, `enter_takes_armed_or_selected`, `esc_blurs_text_field`, `alt_digit_picks_translation_only_in_bible_box`. Testing backend: `f1_in_bible_box_clears_and_types_nothing`, `alt_2_in_bible_box_picks_translation_and_types_nothing`, `ctrl_b_focuses_and_selects_bible_text` (type "John", press Esc, Ctrl+B → focused and the whole text selected), `two_nexts_from_same_base_advance_once` (two `Next` routes before applying the first live update → live version +1 only, and `Ops::outcome_toast(&second_outcome) == None`), `right_arrow_with_dialog_open_does_nothing`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): operator keyboard shortcuts`.

### Task 11: Bible box

**Files:**
- Create: `src/ui/operator/bible.rs`
- Modify: `ui/operator/topbar.slint`

**Interfaces:**
- The top-bar form (`operator.css:33-41`): 42 px tall, max 760 wide, 1.5 px border that turns accent with a `focus-ring` on focus, book icon, the styled `TextInput` `bible-input` from Task 5 (not a `LineEdit`; placeholder "Bible verse — type John 3:16 and press Enter", tooltip "Ctrl+B"), translation chips (`on` = accent fill; an available chip's tooltip is its own name; `off` = opacity .4 with tooltip "`<T>` needs a licensed text source (Settings → Bible)", `operator.js:98`), and `Kbd` "Ctrl B".
- `pub fn submit(op: &Rc<Operator>, reference: String)`:
  1. Trim; empty input does nothing.
  2. `bible/lookup {reference, translation}` → `found`.
  3. `service/add {position: insert_position(..), item: {kind:"scripture", reference: found.label, translation: found.translation, verses: found.verses, per_slide: 0}}`. The item always carries the translation the core returned (`operator.js:330`), never the chip state.
  4. Clear the input, return focus to `keys`, set `selected = armed = id`.
  5. Toast "`<label>` added — press Enter to show it".
  6. Errors go to an error toast with the core's text, and the input keeps its text.
- `pub fn pick(op, index)`: set the translation and save it through `settings/translation`. An `off` translation is still selectable, as in the web; the lookup then fails with the core message. Refocus the input.

- [ ] **Step 1: Write the failing tests** (testing backend + `Ops::inline`): `bible_enter_inserts_after_live_item_and_arms_it`, `bible_enter_appends_when_nothing_live`, `bible_item_uses_translation_from_lookup`, `second_enter_takes_armed_passage_live` (`route("Enter")` → the live cursor is `(new_id, 0)`), `next_key_clears_the_arm`, `unknown_book_shows_core_error_and_keeps_text` ("I don't recognize the book \"Hezekiah\"."), `unavailable_translation_errors_with_core_message` (NIV → "NIV isn't set up yet. It needs a licensed text source; KJV works now."), `available_chip_tooltip_is_its_name`, `translation_choice_survives_restart` (pick NIV, re-read the snapshot from `Data`, the chip is on).
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Bible box with arm-then-Enter flow`.

### Task 12: Toasts and jobs

**Files:**
- Create: `ui/operator/overlays.slint`, `src/ui/operator/overlays.rs`
- Modify: `src/ui/operator/window.rs`

**Interfaces:**
- Toasts: `export struct ToastRow { id: int, text: string, err: bool }` shown bottom-centre, 18 px from the bottom, with an 8 px rise + fade over 180 ms. `pub fn Operator::toast(&self, text: impl Into<String>, err: bool)` removes each toast after 2.6 s, or 6 s for errors, with a `slint::Timer`.
- Jobs: `export struct JobRow { id: string, label: string, percent: int, error: string }` shown bottom-right, 320 px wide. Server jobs come from the snapshot. Client jobs (`Operator::client_job(label) -> JobHandle` with `set_percent`, removed on drop) show local copies, e.g. "Uploading `<name>`", with the percent from `import_path`'s progress callback. An error card has a close button → `job/dismiss {id}`. Its progress bar is a `Bar` with `animated: false` (`.job .bar i` has no transition).

- [ ] **Step 1: Write the failing tests:** `toast_expires_after_2_6s_or_6s_for_errors` (mock time), `job_error_card_dismisses`, `client_job_shows_progress_and_goes_away`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): toasts and job cards`.

### Task 13: Dialog shell and confirm/prompt

**Files:**
- Create: `ui/dialogs/shell.slint`, `ui/dialogs/ask.slint`, `src/ui/dialogs/mod.rs`, `src/ui/dialogs/ask.rs`
- Modify: `ui/operator.slint` (dialog layer), `src/ui/mod.rs`

**Interfaces:**
- `DialogShell` (`operator.css:145-153`): `in property <string> title`, `in property <bool> wide` (wide = min(1180, 96%), normal = min(920, 94%)), max height 88 %, radius 16, `dialog-scrim` with a `TouchArea` that swallows clicks, a head with the title and X, a scrolling body `@children`, an optional footer slot, and its own `FocusScope` (Esc → `close()`; F1–F5 pass to the root through the capture rule in Task 10). Callback `close()`.
- `pub enum DialogKind { Add(AddTab), SongEditor{..}, ItemEdit(Uuid), Services, Screens, Phone, Settings(SettingsTab) }`. `Operator::open_dialog(kind)` swaps the content in place, the same as the web's single `<dialog>`. `close_dialog()` runs the `on_close` hook (Phone stops pairing) and refocuses `keys`.
- `dialog_refresh()` runs on `Data`/`Convert` updates **only for Add, Screens and Phone**, the three dialogs that set the web `dialogRefresh` (`operator.js:388, 827, 859`). Song editor, Item edit and Settings are never refreshed, so a phone or a conversion never wipes what the user typed. Services redraws itself after its own actions (Task 19).
- `ask.slint` + `ask.rs`: `pub fn confirm(op, text, ok_label, cancel_label, danger: bool, done: impl FnOnce(bool) + 'static)` and `pub fn prompt(op, title, initial, done: impl FnOnce(Option<String>) + 'static)`. These are small overlays above any dialog (or above the bare window). Enter = OK and Esc = Cancel. Esc on an ask overlay cancels **only that overlay**; the dialog under it stays open. While any ask is up, `OpState.asks > 0` and `KeyCtx.dialog_open` is true (Task 10). They replace every `confirm()` and `prompt()` in `operator.js`.

- [ ] **Step 1: Write the failing tests:** `confirm_enter_is_ok_esc_is_cancel`, `prompt_returns_text_or_none`, `esc_on_ask_cancels_only_the_ask` (open a dialog, then a confirm; Esc → confirm gone, dialog still open), `right_arrow_with_only_a_confirm_open_does_nothing` (no dialog, a confirm up, → → live version unchanged), `f1_inside_dialog_clears_and_dialog_stays_open`, `esc_closes_dialog_and_refocuses_keys`, `dialog_refresh_only_for_add_screens_phone`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): dialog shell and confirm/prompt overlays`.

### Task 14: File routing and adding files

**Files:**
- Create: `src/ui/operator/files.rs`

**Interfaces:**
- `pub enum Route { Song, Deck, Media }` and `pub fn route(path: &Path) -> Route`: `txt` → Song; `ppt pptx pps ppsx odp` → Deck; anything else → Media (`operator.js:520-545`). `Route::Song` imports with `ImportPurpose::Lyrics`, `Deck` with `PowerPoint`, `Media` with `Media`.
- `pub fn all_images(paths: &[PathBuf]) -> bool` (by extension, `media::IMAGE`).
- `pub fn add_files(op: &Rc<Operator>, paths: Vec<PathBuf>)` follows `uploadFiles(add: true)`:
  - Each file runs as a client job "Uploading `<name>`" and goes through `Ops::import` with the purpose from `route`. Media then runs `addItem({kind:"media", media_id, autoplay:true, end:"stop", looped:false}, quiet)`, a song `addItem({kind:"song", song_id}, quiet)`, and a deck a `watch_deck_job(job)` that adds every new deck when the job ends (driven by `Update::Data`, not polling) and toasts "“T” is ready".
  - Toasts use the web strings: "`<name>` added — converting so it plays smoothly…", "`<name>` added", "Imported “T”", "“T” was already in the library — added a second copy.", and on error "`<file>`: `<err>`".
- `pub fn images_to_deck(op, paths)` follows `imagesToDeck` (`operator.js:565-583`):
  1. A client job "Uploading N picture(s)" ("1 picture" / "N pictures"), its percent set to `round((i+1)/N*100)` after each file.
  2. Sort by file name, numeric-aware, and import each as `Asset`.
  3. Show the **Prompt** "Name these slides" with the first file name without its extension. Cancel (`None`) and a blank answer both give "Slides", and the deck is still made (`prompt(...) || 'Slides'`).
  4. `deck/from-images {title, assets}`, then `addItem({kind:"deck", deck_id})` **not quiet**, so it toasts "Added to the service", then `close_dialog()`.
  5. Any error → error toast; the client job is removed either way.
- `addItem` (Rust, shared with Task 16): `service/add {item, position: null}`, select the new id, then the toast "Added to the service" unless `quiet`.

- [ ] **Step 1: Write the failing tests:** `route_by_extension` (`.txt` → Song with `Lyrics`), `all_images_needs_every_file`, `add_files_media_adds_to_service_quietly` (Ops::inline + a PNG fixture → one service item, toast "pic added"), `add_txt_imports_song_and_adds_it`, `images_to_deck_cancel_still_makes_slides` (prompt → `None` → a deck titled "Slides" is in the service, toast "Added to the service", dialog closed), `images_to_deck_sorts_numerically` (`2.png`, `10.png` → assets in that order).
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): add files and pictures to the service`.

### Task 15: Desktop file drop and native file dialogs

**Files:**
- Create: `src/platform/winit_events.rs`
- Modify: `Cargo.toml` (`rfd`; slint gains feature `raw-window-handle-06`), `src/ui/operator/files.rs`, `src/ui/operator/window.rs`, `ui/operator/overlays.slint` (drop hint)

- [ ] **Step 0: Check C3 first (spec §12: checks come before dependent work).** In a throwaway example, install a `WinitWindowAccessor::on_winit_window_event` filter on a Slint window and log `HoveredFile`, `HoveredFileCancelled` and `DroppedFile` while dragging 1 file and then 3 files from Explorer. Record the result in the checks doc C3 row before writing the code below. Background for the design:
  - The winit 1.18.1 backend does not forward `DroppedFile`/`HoveredFile` to `DropArea` (no matches in the `i-slint-backend-winit-1.18.1` source), so the window-event filter is required.
  - `on_winit_window_event` replaces any earlier filter (`window_event_filter.set(...)`, `lib.rs:1224`), which is why the shared `Dispatcher` below is needed.
  - If C3 fails, ship the native file dialogs only and leave drag-and-drop for later (spec §12).

**Interfaces:**
- `Cargo.toml`: `rfd = { version = "=X.Y.Z", default-features = false, features = ["xdg-portal", "tokio", "common-controls-v6"] }`. `rfd` is not in the local registry, so the version is not verified here: in Step 1, run `cargo add rfd --no-default-features -F xdg-portal,tokio,common-controls-v6`, check that the resolved version exists and builds on Windows, and pin it with `=` (the async backend on Linux needs `xdg-portal` + `tokio`). Then `cargo build` (Tauri) and `cargo test` before going on.
- Drop hint: a full-window `drop-scrim` with a dashed accent box, "Drop to add to the service" and "Videos, pictures, music, PowerPoint, SongSelect lyric files".
- `platform::winit_events::Dispatcher`: the single `on_winit_window_event` filter for the operator window, with `add(f)` for each user, because Slint allows only one filter per window.
  - `HoveredFile` shows the hint and `HoveredFileCancelled` hides it.
  - `DroppedFile(p)` collects paths, and a `Timer::single_shot(Duration::ZERO)` handles the batch.
  - If there is more than one file and every file is an image, show **Confirm** "Make N pictures into one set of slides?" with the buttons **One slide set** / **Separate pictures** (the native wording of the web OK/Cancel) → `images_to_deck` or `add_files`. Otherwise `add_files`.
- `pub fn pick(op, filter: Filter, multi: bool, done: impl FnOnce(Vec<PathBuf>) + 'static)` uses `rfd::AsyncFileDialog` with `set_parent(&ui.window().window_handle())`, run through `slint::spawn_local`. It remembers the last folder in memory for the session. The filters are the web `accept` lists: media `video/*,audio/*,image/*` plus `.mkv .mov .wmv .avi .mts .m2ts .mpg .mpeg .flv .vob .wma .flac .aiff` (expanded to `media::VIDEO/AUDIO/IMAGE`), PowerPoint `.pptx .ppt .ppsx .pps .odp`, lyrics `.txt`, pictures `media::IMAGE`, and pictures+videos `media::IMAGE + media::VIDEO`.

- [ ] **Step 1:** Add `rfd` as above; `cargo build` and `cargo test` pass.
- [ ] **Step 2: Write the failing tests:** `drop_batch_collects_files_from_one_turn` (feed three `DroppedFile` events to the pure batcher → one batch of 3), `dispatcher_calls_every_handler`, `many_pictures_dropped_ask_once`.
- [ ] **Step 3:** FAIL → **Step 4:** implement.
- [ ] **Step 5:** `cargo test` passes.
- [ ] **Step 6: Manual check C3 (Windows):** drag 1 file, then 3 pictures, then a `.pptx` from Explorer onto the native operator. The hint shows, the paths arrive and the items are added. Update the checks doc C3 row. macOS and Linux are left for the Plan 4 manual checklist.
- [ ] **Step 7:** Commit `feat(ui): desktop file drop and native file dialogs`.

### Task 16: Add dialog (Song, Bible, Announcement)

**Files:**
- Create: `ui/dialogs/add.slint`, `src/ui/dialogs/add.rs`

**Interfaces:**
- Add dialog (title "Add to service", normal width): five tiles (`Song/Lyrics`, `Bible/Verses`, `Video & media/Videos, pictures, music`, `Slides/PowerPoint, pictures`, `Announcement/Your own text`) above the current tab. On `dialog_refresh` the current tab's list is rebuilt (`operator.js:388`).
  - `searchList`: a `LineEdit` focused on open, a `FilterModel` over the rows (case-insensitive substring of the row's JSON), and "Nothing matches." / "Nothing here yet.".
  - **Song tab:**
    - **New song** opens the song editor with `add_after` (Task 17).
    - **Import SongSelect lyrics (.txt)** runs `pick(.txt, multi)` → `add_files`.
    - The list is sorted by title. The placeholder is "Search N song(s)…", and the subtitle is `writers · CCLI #n`, or the first lyric line (≤ 80 characters, skipping `slides::section_label` lines).
    - **Edit** opens the song editor. **Add** runs `addItem({kind:"song", song_id})` and closes the dialog.
  - **Bible tab:**
    - Label "Reference (`<translation>`)" and placeholder "e.g. John 3:16-18, Psalm 23, Romans 8:28,31-39" at 16 px.
    - Enter or **Add verses** runs the lookup, then `addItem(…, quiet)` appended at the end (with `found.translation`), then the toast "`<label>` added", then closes.
    - Tip text copied from `operator.js:438`.
  - **Announcement tab:**
    - Title (placeholder "e.g. Welcome, Announcements, Offering", focused) and Text (placeholder "Type your text. Leave an empty line to start a new slide.").
    - **Add announcement**: an empty title gives the error toast "Give it a title.". Otherwise `addItem({kind:"text", title, body})` and close.

- [ ] **Step 1: Write the failing tests:** `add_dialog_opens_on_song_tab_from_add_button`, `song_tab_search_filters_and_empty_messages`, `song_row_subtitle_falls_back_to_first_lyric`, `add_song_adds_and_closes`, `bible_tab_appends_at_end_and_toasts_label`, `announcement_needs_a_title`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): Add dialog song, Bible and announcement tabs`.

### Task 17: Add dialog Media and Slides tabs, and the song editor

**Files:**
- Create: `ui/dialogs/add-media.slint`, `ui/dialogs/song-editor.slint`, `src/ui/dialogs/add_media.rs`, `src/ui/dialogs/song_editor.rs`

**Interfaces:**
- **Media tab:**
  - **Upload files** runs `pick(media, multi)` → `add_files`.
  - Hint "Any video format works. Unusual ones are converted once, automatically."
  - Newest first, placeholder "Search N file(s)…".
  - Each row has a thumbnail (image decoded at 96 px; video/audio show an icon until Plan 4), the name, the kind name (or, for video/audio while `snapshot.player_missing`, "The video player is missing — reinstall RustyPresenter") and a status `Chip` (Ready ok, Converting N% warn, Needs converter warn + tooltip reason, Failed bad + tooltip reason).
  - **Convert** (unconverted or failed only) runs `media/convert` → "Converting…". The trash button (tooltip "Delete from library", `operator.js:467`) asks **confirm** "Delete “name” from the library?" → `media/delete`. **Add** runs `addItem({kind:"media", media_id, autoplay:true, end:"stop", looped:false})` and closes.
- **Slides tab:**
  - **Import PowerPoint** runs `pick(PowerPoint)` → `add_files`. **Make slides from pictures** runs `pick(pictures, multi)` → `images_to_deck`.
  - The Google Slides note is copied verbatim. Placeholder "Search slides…".
  - Each row: a thumbnail of the first slide, the title, "N slide(s)", a trash button with **confirm** "Delete “T”?" → `deck/delete`, and **Add** → `addItem({kind:"deck", deck_id})` and close.
- **Song editor** (wide, two columns), title "Edit song" or "New song". It is never rebuilt by `dialog_refresh`:
  - Fields Title / Writers / CCLI song # / Copyright / Lyrics (`TextEdit`, help "— section names on their own line; an empty line inside a section starts a new slide"), and Order (optional), placeholder "Leave empty to use the written order".
  - `+ <label>` chips for each unique section from `slides::parse_sections`; a click appends the label to Order.
  - Preview: 250 ms after the last edit, Rust builds a `Song` and calls `slides::song_slides(&song, &settings)` (the real code; the JS mirror is not ported). Each slide is a `Thumbs` request with the settings background and theme, captioned with its label. Empty text: "Slides appear here as you type. Put section names like “Verse 1” or “Chorus” on their own line."
  - New: under Order, a faint warning "Not in the lyrics: X, Y" for order names that match no section (the core drops them silently).
  - Footer: **Delete song** (danger, existing songs only; **confirm** "Delete “T” from the library?" → `song/delete`), **Cancel**, and **Save and add to service** (with `add_after`) or **Save song**.
  - Save trims every field except the lyrics and splits Order on `,` (trimmed, empties dropped) → `song/save` → with `add_after`, `addItem({kind:"song", song_id})`, otherwise toast "Song saved" → close.
  - Focus goes to Lyrics when editing and to Title when new.
- `pub fn order_warnings(lyrics: &str, order: &str) -> Vec<String>` (pure).

- [ ] **Step 1: Write the failing tests:** `media_rows_newest_first_with_status_chips`, `media_row_says_player_missing`, `delete_media_asks_then_calls_core` (confirm → No: no call; Yes: deleted; in a service → toast "This is in a service. Remove it from the service first."), `convert_button_only_for_unconverted_or_failed`, `deck_rows_show_slide_count`, `song_editor_preview_uses_core_slides` (lyrics "Verse 1\na\nb\nc" with 2 lines per slide → 2 previews labelled "Verse 1 (1/2)" and "Verse 1 (2/2)"), `section_chips_append_to_order`, `order_warnings_lists_unknown_names`, `save_new_song_with_add_after_adds_to_service`, `blank_title_shows_core_error_and_stays_open`, `delete_song_in_service_shows_core_error`, `esc_closes_editor_without_saving`, `song_editor_keeps_typed_text_across_a_data_update`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): media and slides tabs, song editor with live preview`.

### Task 18: Item edit and background picker

**Files:**
- Create: `ui/dialogs/item-edit.slint`, `ui/dialogs/background-picker.slint`, `src/ui/dialogs/item_edit.rs`, `src/ui/dialogs/background.rs`

**Interfaces:**
- `BackgroundPicker`: `in property <[BgChoice]> choices` (`{label, swatch: color, image: image, kind: string, on: bool, tooltip: string}`), with callbacks `pick(int)`, `custom-color(color)` and `upload()`. Rust: `pub fn choices(media: &[MediaFile], allow_default: bool) -> Vec<Choice>`. The order is "Theme default" (when allowed), the six swatches `#000000 #0b1a33 #1d2b1f #2b1530 #3a2412 #ffffff` (data values in Rust, not `.slint` literals), a custom colour, every **ready** image or video (video names prefixed "▶ "; tooltip = the media name; video choices show black until Plan 4, see Task 7), and "+ Upload…". `pub fn selected(choices, value: &Option<Background>) -> Option<usize>` compares by equality. The custom colour uses a small built-in colour grid + hex `LineEdit` (Slint 1.18 has no colour dialog), tooltip "Pick any color" (`operator.js:704`). **Upload…** runs `pick(images+videos, multi: true)` and imports **every** chosen file with `Ops::import(Media)` in turn (`operator.js:711-713`), then redraws and toasts "Added to the media library — pick it here".
- Item edit (title = the item title), working on a clone of the `ServiceItem`. It is never rebuilt by `dialog_refresh`. Sections by kind are copied from `operator.js:722-782`:
  - song: note + **Edit song**, or "This song was deleted from the library."
  - scripture: "Verses per slide" `Segmented` Automatic / 1 verse … 4 verses.
  - text: Title and Text (help "— an empty line starts a new slide").
  - video: checkbox "Start playing as soon as it is shown" + "When the video ends" Segmented (Clear the screen / Stay on last frame / Play again (loop)).
  - audio: "Repeat until stopped" + the F4 note.
  - media: "Name in library" + a not-ready status note.
  - deck: "Name".
  - song/scripture/text: Background picker (`allow_default`).
  - every kind: "Stage notes" (placeholder "Shown on the stage display only").
  - Footer: **Remove from service** (danger, no confirm), **Cancel**, **Save** → rename if changed (`media/rename` / `deck/rename`, trimmed and non-empty) then `service/update` → close + "Saved". If `service/update` fails, the dialog stays open with the error toast.

- [ ] **Step 1: Write the failing tests:** `background_choices_order_and_ready_only`, `selected_choice_matches_current_value`, `upload_imports_every_chosen_file`, `scripture_per_slide_saved`, `video_end_action_saved`, `text_item_title_and_body_saved`, `rename_media_then_update`, `item_edit_keeps_typed_text_across_a_data_update`, `save_on_removed_item_shows_that_item_is_gone_and_stays_open`, `remove_from_service_closes`.
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): item edit and background picker`.

### Task 19: Services, Screens and Phone dialogs

**Files:**
- Create: `ui/dialogs/services.slint`, `ui/dialogs/screens.slint`, `ui/dialogs/phone.slint`, `src/ui/dialogs/services.rs`, `src/ui/dialogs/screens.rs`, `src/ui/dialogs/phone.rs`

**Interfaces:**
- **Services** (title "Services"):
  - **New empty service** runs **prompt** "Name for the new service" (default "Service `<local date>`") → `service/new {name}` → close. Cancel does nothing.
  - **Copy this service** runs **prompt** "Name for the copy" (same default "Service `<local date>`", `operator.js:789`) → `service/new {name, copy_from}` → close. Cancel does nothing.
  - Rows show the name + an `Open` chip on the current service and "N item(s)". **Rename** runs **prompt** "Rename service" (default the current name) → if the answer is non-empty, `service/rename` (untrimmed, as in the web); then the Services dialog is rebuilt and stays open, **even when the prompt was cancelled** (`operator.js:792`). Trash (only with more than one service) asks **confirm** "Delete the service “N”? (Songs and media stay in the library.)" → `service/delete` → the dialog is rebuilt and stays open (`:793`). **Open** (not current) runs `service/select` → close.
  - The local date uses `chrono::Local::now().format("%-m/%-d/%Y")` (en-US, as the web shows on church PCs).
- **Screens** (title "Screens"): two cards, room "Room screen" / "What the congregation sees: projector or TV." and stage "Stage display (optional)" / "For singers and the pastor: current and next lyrics, notes and a clock.".
  - Status chip from `room_connected` / `stage_connected` and `windows`: Showing (ok), Starting… (warn) or Off (`operator.js:804, 809`).
  - The monitor picker is one button per `snapshot.monitors` (`label`). The default is `default_pick`: the saved monitor if it is still present, else the first non-primary one, else `None`. With one monitor the picker is replaced by the "Only one screen is connected…" note.
  - **Show on this screen** / **Move / reopen here** is **disabled when there is no pick** (`operator.js:813`); it runs `display/open {role, monitor}` → "`<title>` opened" → `dialog_refresh` after 1.5 s. **Turn off** appears **only while that role's window is open** (`:814`) and runs `display/close`. **Test in a window** runs `display/open {role, monitor: null}`.
  - Footer: "RustyPresenter remembers your choice and reopens the screens next time." + **Refresh**, which calls `Operator::request_refresh()` so the follower thread re-reads the whole snapshot including `App::monitors()`, then redraws when it arrives (`:823`). It never calls `monitors()` on the Slint thread.
  - Pure: `pub fn default_pick(monitors: &[Monitor], saved: Option<&str>) -> Option<String>`.
- **Phone** (title "Phone remote", `operator.js:858`):
  - The note, verbatim (`:835-836`): "Control the slides from any phone on the church Wi-Fi. The phone must be on the same network as this computer." followed by "Address: " and the `lan_url` in a `CopyText`, or " (No network found — connect this computer to Wi-Fi.)".
  - **Pair a phone** runs `pair/start` → the QR comes from the returned SVG through `slint::Image::load_from_svg_data(svg.as_bytes())`, then the two steps verbatim (`:850-851`): "1. On the phone, open the camera and scan the code (or type the address)." and "2. Enter this code on the phone:", a large PIN, and the countdown "Code works for N more seconds" → "Code expired — click Pair a phone again." (a 1 s `Timer`).
  - "Paired phones" list with **Remove** (danger, no confirm) → `phone/revoke`. Empty: "None yet.".
  - Closing the dialog stops the countdown and calls `pair/stop`, ignoring errors.
  - `dialog_refresh` redraws only while no PIN is showing.

- [ ] **Step 1: Write the failing tests:** `new_service_prompt_creates_and_selects`, `copy_service_prompt_defaults_to_dated_name`, `rename_service_sends_untrimmed_name`, `cancelled_rename_keeps_services_open`, `delete_service_keeps_services_open`, `delete_last_service_is_hidden` (trash absent with one service), `open_other_service_selects_it`, `default_pick_prefers_saved_then_non_primary`, `one_monitor_nothing_saved_disables_show` (`default_pick` → `None`, the button is disabled), `turn_off_only_while_window_open`, `one_monitor_shows_plug_in_note`, `screens_status_chips` (guard → Showing; in `windows` → Starting…; stage uses `stage_connected`), `refresh_requests_a_new_read_with_monitors`, `phone_dialog_title_and_address_text`, `pair_shows_six_digit_pin_and_qr`, `closing_phone_dialog_stops_pairing` (`app.pairing` is `None` afterwards), `countdown_expires_with_web_text` (mock time 120 s).
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): services, screens and phone pairing dialogs`.

### Task 20: Settings (Look, Songs, Bible, Video, Backups, Help)

**Files:**
- Create: `ui/dialogs/settings.slint`, `ui/dialogs/settings-look.slint`, `src/ui/dialogs/settings.rs`
- Modify: `Cargo.toml` (`arboard = { version = "=3.7.0", default-features = false }` for the Copy button; 3.7.0 is the version already in `Cargo.lock`). Then `cargo build` (Tauri) and `cargo test`.

**Interfaces:**
- A wide dialog (title "Settings") editing a clone of `Settings`. Tabs: Look, Songs, Bible, Video, Backups, Help, About (Task 21). It is never rebuilt by `dialog_refresh`. Footer **Cancel** / **Save settings** → `settings` → close + "Settings saved".
- **Save keeps live fields:** before sending, Save copies `translation` and `displays` from the **latest** snapshot into the edited clone, so a translation picked meanwhile (through `settings/translation`, from this or another operator) or a monitor chosen in Screens is never undone by a Settings save.
- **Look:**
  - Font `ComboBox` (the 20 fonts in `operator.js:880`), Lyrics size `Slider` 40–150 with help "(biggest; long lines shrink to fit)", Scripture size 36–120, Text color (the picker from Task 18), Text position Segmented Middle / Lower third, the Shadow and ALL CAPS checkboxes with the web labels.
  - When the chosen font is not installed (the preview render reports `font_fallback`, `render/cache.rs`), a note under the font box says "“`<font>`” isn't installed on this computer, so slides use Noto Sans." (native spec §4; Product decision 8).
  - Logo: a 44 px preview or "No logo (Logo button shows black)", **Choose logo** (pick an image → `import(Asset)` → `logo = asset`) and **Remove**.
  - Right column: a 16:9 preview from `Thumbs` of the sample text "Amazing grace how sweet the sound\nThat saved a wretch like me" with the footer "Amazing Grace · John Newton · Public Domain", re-rendered 150 ms after each change, plus the "Default background" picker (`allow_default: false`). Switching tabs drops the preview image; coming back to Look renders it again from the current edits (`operator.js:937`), never showing a stale image.
- **Songs:** Lines per slide Segmented 1–6, Song credits (On first slide / On last slide / Off), Church CCLI license #.
- **Bible:** mirror the current web Settings → Bible (after the `bible-esv` merge): per-service key rows (`bible/key {provider,key}`), Bible file import (`bible/import/{ABBR}`, raw body, through an `rfd` file pick), remove (`bible/import-remove {translation}`), the per-translation status chips and credits from `snapshot.bible_sources` / `bible_providers`. Pasted verses (`bible/paste {reference,translation,text}`) belong to the Bible box (Task 11) when the web offers them there.
- **Video:** the tab stays until Plan 4 removes FFmpeg, and shows one of the two web states (`operator.js:912-915`) depending on `snapshot.ffmpeg`, copied verbatim. Without FFmpeg it includes `winget install --id Gyan.FFmpeg -e` in a `CopyText` with **Copy** (→ `arboard::Clipboard::new()?.set_text(..)`, because Slint 1.18 has no public clipboard-write API → "Copied") and **Check again**, which requests a fresh snapshot, redraws the tab and toasts "Converter found!" or "Not found yet — restart RustyPresenter after installing." (`:920`).
- **Backups:**
  - The note with `<data_folder>\backups` in a `CopyText`. **Back up now** (primary) runs `backup/now` (a blocking-pool job, Task 1) → "Backup made".
  - Rows show `created` formatted as in `operator.js:924` ("2026-11-01 at 14:05 UTC") and "N songs · N services · N files". **Restore** asks **confirm** "Restore this backup? Your current songs and services are replaced (a copy of them is kept)." → `backup/restore` → close + "Backup restored". Empty: "No backups yet.".
- **Help:** the keyboard table from `operator.js:930` with the ↓/↑ wording change (Product decision 7): "→ ↓ Space Page Down" Next, "← ↑ Page Up" Back, then the other web rows, plus "Alt + 1–6 — Bible translation (in the Bible box)" and the tip "Tip: clicking any slide puts it on the screen right away. The red outline shows what the room sees." (version and data folder move to About).
- Pure: `pub fn backup_label(created: &str) -> String`.

- [ ] **Step 1: Write the failing tests:** `save_settings_round_trips_theme_and_lines`, `save_settings_keeps_translation_and_displays_changed_meanwhile` (open Settings, change the translation through `settings/translation` and `displays.room` through the core, apply the snapshot, Save → both kept), `cancel_discards_changes`, `settings_keeps_typed_text_across_a_data_update`, `look_preview_rerenders_after_change` (mock time 150 ms → the preview image changes), `look_preview_rebuilt_on_tab_return`, `missing_font_shows_note` (font "No Such Font" → the note shows; "Noto Sans" → hidden), `check_again_toasts_found_or_not`, `backup_label_formats_like_web`, `restore_asks_then_restores`, `help_lists_every_bound_key` (compare the Help rows with the `keys::route` table, so the list cannot drift from the real bindings), `bible_tab_shows_sources_keys_and_import` (KJV/WEB/BSB ready; saving a key and importing a file go through the actions above).
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(ui): settings dialog`.

### Task 21: Settings → About, AboutSlint and THIRD-PARTY-LICENSES

**Files:**
- Create: `ui/dialogs/settings-about.slint`, `src/ui/dialogs/about.rs`, `about.toml`, `about.hbs` (plain-text template), `THIRD-PARTY-LICENSES.txt` (generated, committed), `packaging/licenses/native.txt` (libmpv LGPL-2.1+ / FFmpeg LGPLv2.1 notice with the source location, pdfium BSD/Apache, Noto Sans OFL)
- Modify: `.github/workflows/ci.yml` (a job that regenerates the file and fails if it differs)

**Interfaces:**
- The About tab: `AboutSlint { }` (imported from `std-widgets.slint`), then "RustyPresenter `<version>`", "Your data: " + `<data_folder>` in a `CopyText`, "Video: GPU (OpenGL)" or "Video: software drawing" from `video::surface::DrawMode` (or "Video player missing" when `Mpv::load` failed), the libmpv version (`mpv-version` + `ffmpeg-version` properties read once at startup; empty when missing), the line "Video playback uses libmpv (LGPL-2.1+) and FFmpeg libraries under the LGPLv2.1 — source: `<release URL>`", and **Show licenses**, which opens a scrolling read-only `TextEdit` with the embedded licences (`include_str!("../../../THIRD-PARTY-LICENSES.txt")` followed by `packaging/licenses/native.txt`).
- `pub struct AboutInfo { version, data_folder, video: String, mpv: String }` and `pub fn about_info(snap, info: &VideoInfo) -> AboutInfo` (`VideoInfo` from `OperatorScreens::video_info`).
- `cargo about generate about.hbs -o THIRD-PARTY-LICENSES.txt`. Accepted licences in `about.toml`: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, OFL-1.1, BSL-1.0, CC0-1.0, and the Slint licence `LicenseRef-Slint-Royalty-free-2.0` (exact SPDX id: check `cargo about` output once).

- [ ] **Step 1: Write the failing tests:** `about_shows_aboutslint` (`ElementHandle::find_by_element_type_name(&ui, "AboutSlint")` finds one), `about_info_reports_software_mode`, `about_info_reports_missing_player`, `licenses_text_mentions_slint_and_libmpv` (the embedded text contains "Slint" and "libmpv").
- [ ] **Step 2:** FAIL → **Step 3:** implement. Install `cargo-about` (pin the version in the CI job), generate the file and commit it.
- [ ] **Step 4:** `cargo test` passes and the CI licence job passes locally (`cargo about generate … && git diff --exit-code THIRD-PARTY-LICENSES.txt`).
- [ ] **Step 5:** Commit `feat(ui): Settings About with AboutSlint and third-party licenses`.

### Task 22: UI flow tests on the testing backend

**Files:**
- Create: `tests/ui_operator_flows.rs` (one binary; the same `thread_local!` init guard as `ui::ui_test_init`; a temp data folder per test through `rustypresenter::app::App::new` over `Store::open(tempdir)`)
- Modify: `src/ui/mod.rs`: `#[doc(hidden)] pub mod test_support { pub fn operator_for_tests(app: Arc<App>) -> Rc<Operator> }`, plain `pub` (integration tests cannot see `#[cfg(test)]` items, and `test_app` is `#[cfg(test)]`, `app.rs:614`). It opens the operator with `Ops::inline` and a fake `OperatorScreens`. No self-dev-dependency or extra feature.

**Interfaces:**
- Each flow drives the window only through element ids and the root `key` callback, then asserts on `app.live_view()` / `app.data` and on visible properties. These are the main flows from spec §13 and the v1 §14.2 Navigation row:
  1. `select_item_shows_its_slides_without_going_live`: click `row-1`; `slides-title` = the item title; the live version is unchanged.
  2. `next_and_back_move_the_live_slide`: `next-btn` ×2, `prev-btn` ×1; the cursor is at slide 1; `up-next` shows the next label.
  3. `next_ignores_selection_and_follows_service_order`: three songs A, B, C; A live, C selected; → mid-item stays in A; at A's last slide → takes B live (the next in service order), not C.
  4. `clear_all_from_button_and_f1`: both leave an empty frame; F1 works with focus in `bible-input`.
  5. `bible_lookup_then_enter_takes_it_live`: type "John 3:16" into `bible-input` (`set_accessible_value`), submit, then press Enter on the root; the live item title is "John 3:16 (KJV)" and the room cursor is on slide 0.
  6. `add_song_through_dialog`: `add-btn` → **New song** → fill the title and lyrics → **Save and add to service**; the service has 1 song item and the dialog is closed.
  7. `reorder_by_drag`: three items; drag row 0 onto row 2; the order is 1, 2, 0 (`to: 2`).
  8. `dialog_blocks_presentation_keys`: open Settings, press →; the live version is unchanged; press F1 → cleared.
- [ ] **Step 1:** Write the tests. Any that fail now point at real gaps from Tasks 5–20.
- [ ] **Step 2:** Fix the glue until they pass. Do not weaken the tests.
- [ ] **Step 3:** `cargo test` passes on Windows. Push: CI runs them on macOS and Linux (C4 rows for those OSes; record the results).
- [ ] **Step 4:** Commit `test(ui): operator main flows on the Slint testing backend`.

### Task 23: Switch the native binary to the native operator

**Files:**
- Modify: `src/bin/native.rs`, `src/ui/screens.rs` (role `operator`), `README.md`, `PROJECT_PLAN.md`, `docs/superpowers/checks/2026-native-checks.md`

**Interfaces:**
- `native.rs` opens `Operator::open(app, Ops::new(app, rt.handle()), screens.clone() as Rc<dyn OperatorScreens>)` on the Slint thread after the room and stage reopen from settings, then calls `NativeScreens::set_operator(Rc::downgrade(&operator))` (the operator holds the screens, so the back-reference is a `Weak`). It drops the "open the browser" step and the `--no-browser` flag (the flag is accepted and ignored for one release). The web operator page is still served for the Tauri app until Plan 4.
- `NativeScreens::open("operator", _)` upgrades the `Weak<Operator>` and calls `Operator::raise()`. This is how the second-launch focus request (`display/open {role: "operator"}`) brings the window forward (spec §8.4).
- Closing the operator window asks nothing (as today) and quits: room and stage close, `slint::quit_event_loop()` runs, and the process exits 0.
- Window placement: the operator opens on the primary monitor, or on the monitor not used by the room (`monitors::pick` rules in reverse), maximised.

- [ ] **Step 1: Write the failing tests:** `display_open_operator_raises_window` (testing backend: `screens.open("operator", None)` → `Operator::raise` called once, through a counter in test builds), `operator_avoids_room_monitor` (pure helper `operator_monitor(monitors, room: Option<&MonitorId>)`).
- [ ] **Step 2:** FAIL → **Step 3:** implement.
- [ ] **Step 4:** `cargo build --release --bins` and `cargo test` pass.
- [ ] **Step 5: Manual check (Windows, recorded in the checks doc as "Native operator").** Run the release build on the church-style setup:
  - Build a service with the Add dialog: a song (new + SongSelect `.txt` import), a scripture via Ctrl+B, a video, a picture, a PowerPoint, an announcement.
  - Reorder by drag, edit an item, change settings, pair a phone.
  - Run the whole service with the keyboard only (→ Space PageDown ← Enter F1–F5).
  - Check the live preview of video while it plays.
  - Drop files from Explorer.
  - Launch a second copy: the operator comes to the front.
  - Compare side by side with the web operator in the Tauri app: same layout, wording and colours.
  - Unplug/replug and 3-display tests wait for the church laptop.
  - macOS and Linux: added to the Plan 4 per-platform manual checklist.
- [ ] **Step 6:** Update `README.md` (how to run the native app), `PROJECT_PLAN.md` (NATIVE-P3 status, handoff to Plan 4), and the checks doc. Commit `feat: native binary opens the native operator window`.

---

**After this plan:** write Plan 4 (cross-platform imports with LibreOffice/pdfium and mpv probing/thumbnails, supervisor and crash recovery, packaging with cargo-packager and the LGPL libmpv CI job, Tauri/WebView2/FFmpeg removal). These depend on that work or are left for later:
- Video frames everywhere this plan shows black: video slide thumbnails, text-slide thumbnails over a video background, video background choices in the picker, and video/audio rows in Add → Media.
- The "update existing or keep both" SongSelect duplicate prompt (v1 spec §9.2).
- Removing the Settings → Video FFmpeg tab.
- Audio output device selection in Settings and the "selected device missing" notice (native spec §5, v1 §6.4); `Settings` has no audio device field today (`src/model.rs:184-193`).
