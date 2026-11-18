# Livestream Scenes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The livestream changes scene by itself (camera, camera + words, full screen, screen + camera, starting soon, ending, hold) from what is live, with one-click override.

**Architecture:** The app composes the stream picture itself in the recorder's writer loop from four RGBA sources (room, camera, words, card) and hands FFmpeg one plain input. A worker picks the scene ten times a second (pure `choose`) and draws the words and card pictures; the camera worker keeps its newest RGBA frame in memory. Manual choices live on `App` so operator buttons and macro steps share them.

**Tech Stack:** Rust 2024, Slint 1.18, FFmpeg child process, existing `render::cache::draw_key` for words and cards.

**Spec:** `docs/superpowers/specs/2026-11-15-livestream-scenes-design.md`

## Global Constraints

- Nothing may stop the stream or recording: a camera that stops, a card picture that fails to load, a words picture of the wrong size all fall back (spec "Anti-goals").
- Scene change fade: 0.5 s. Corner picture: a quarter of the frame's width, bottom right, margin `w / 40` (as today's `picture_in_picture`).
- Every user-visible string through `tr!`; run `cargo run --example i18n_extract` after adding strings (tests `i18n_catalogs` check it).
- Saved data loads leniently: unknown scene names load as `Auto`/default (`#[serde(other)]` last variant pattern used in `automation::preservice::PreServiceEnd`).
- Checking a change: `C:\Users\jakes\OneDrive\Documents\RustyPresenter\quick-check.cmd <filter> < NUL` (or `cargo test --profile quick --offline --lib -- <filter>` from the worktree when quick-check is busy). Never unfiltered.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Camera unplugged mid-stream while a camera scene is out → stream shows Full screen, no stall; back to the camera when it returns. (Task 2 test `a_missing_camera_shows_the_room`, Task 4 test `the_camera_slot_goes_empty_when_pictures_stop`.)
2. Room and stream at different sizes (room 4:3 projector, stream 16:9) → room picture fitted with bars before composing; the sink already fits (`FrameSink::put_raw`), so compose assumes equal sizes and asserts lengths, never panics (Task 2 test `inputs_of_the_wrong_size_are_left_out`).
3. Manual click then the same item stays live for an hour → the click holds; another item taken live → Auto again; Go Live stopped and started → override cleared (Task 1 tests, Task 5 test).
4. Freeze pressed during a fade → the frame going out stays, fade resumes from where it was when released is not required; output must stay the frozen frame (Task 3 test `freeze_keeps_the_frame_even_mid_fade`).
5. Card picture file deleted or unreadable → card falls back to its words (Task 3 test `a_card_picture_that_cannot_load_shows_its_words`).

---

### Task 1: Scenes and the automatic choice

**Files:**
- Create: `src/stream/scene.rs`
- Modify: `src/stream/mod.rs` (add `pub mod scene;`)

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")] pub enum Scene { Camera, CameraWords, Screen, ScreenCamera, StartingSoon, Ending, Hold }`; `Scene::ALL: [Scene; 7]` in button order; `Scene::label(self) -> String` (tr: "Camera", "Camera + words", "Full screen", "Screen + camera", "Starting soon", "Ending", "Hold"); `Scene::needs_camera(self) -> bool` (Camera, CameraWords, ScreenCamera).
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct LiveInfo { pub pre_service: bool, pub cursor_item: Option<Uuid>, pub last_item: bool, pub media_playing: bool, pub slide: SlideKind }` with `pub enum SlideKind { #[default] None, Words, Picture }` (Text with non-blank text → Words; Image/Design → Picture; blank text or no slide → None).
  - `pub struct AutoPrefs { pub on: bool, pub presentation_camera: bool }`.
  - `#[derive(Clone, Copy, Debug, Default, PartialEq)] pub struct Choice { pub scene: Option<Scene>, pub item: Option<Uuid> }` — `scene: None` is Auto. `Choice::pick(scene: Option<Scene>, item: Option<Uuid>) -> Choice`.
  - `pub fn choose(info: &LiveInfo, prefs: AutoPrefs, choice: Choice, last: Scene) -> Scene` — manual choice wins while `choice.item == info.cursor_item`; then if `!prefs.on` → `last`; else the spec's rules 1–7 in order.
  - `pub fn effective(scene: Scene, camera: bool) -> Scene` — `needs_camera && !camera` → `Screen`.
  - `pub fn lit(choice: Choice, info: &LiveInfo) -> Option<Scene>` — the manual scene while it still holds, else `None` (Auto lit).

- [ ] **Step 1: Write the failing tests** in `scene.rs` `mod tests`:
  - `starting_soon_before_anything_and_while_the_countdown_runs`: `LiveInfo::default()` → `StartingSoon`; `pre_service: true` with a Words slide → `StartingSoon`.
  - `words_get_the_camera_and_words`: cursor Some, `slide: Words` → `CameraWords`.
  - `a_video_goes_out_full`: `media_playing: true, slide: Words` → `Screen`.
  - `pictures_follow_the_presentation_setting`: `slide: Picture` → `Screen`; with `presentation_camera: true` → `ScreenCamera`.
  - `nothing_on_the_slide_is_the_camera`: cursor Some, `slide: None`, `last_item: false` → `Camera`; `last_item: true` → `Ending`.
  - `a_click_holds_until_the_next_item`: choice `Hold` on item A, info cursor A → `Hold`; cursor B → auto result; `lit` returns `Some(Hold)` then `None`.
  - `auto_off_keeps_the_last_scene`: `on: false`, last `Camera`, Words slide → `Camera`.
  - `no_camera_falls_back_to_the_room`: `effective(CameraWords, false) == Screen`, `effective(StartingSoon, false) == StartingSoon`, `effective(Camera, true) == Camera`.
  - `scene_names_are_kept_by_name`: `serde_json::to_value(Scene::CameraWords) == json!("camera_words")`.

- [ ] **Step 2: Run** `cargo test --profile quick --offline --lib -- stream::scene` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** the same — Expected: PASS.
- [ ] **Step 5: Commit** `stream: scenes and the automatic choice`.

### Task 2: Putting the picture together

**Files:**
- Create: `src/stream/compose.rs`
- Modify: `src/stream/mod.rs` (`pub mod compose;`)

**Interfaces:**
- Consumes: `Scene` (Task 1).
- Produces:
  - `pub struct Inputs<'a> { pub size: (u32, u32), pub room: &'a [u8], pub camera: Option<&'a [u8]>, pub words: Option<&'a [u8]>, pub card: Option<&'a [u8]> }` — all RGBA `size`; `camera` opaque RGBA (shape already kept by its FFmpeg), `words`/`card` premultiplied RGBA. A slice of the wrong length counts as `None`.
  - `pub fn compose(scene: Scene, i: &Inputs, out: &mut [u8])` — Camera: camera (else room). CameraWords: camera (else room) then words over. Screen: room. ScreenCamera: room, camera scaled into `corner_rect` (nearest pixel). StartingSoon/Ending/Hold: card over black (no card → black).
  - `pub fn corner_rect((w, h): (u32, u32)) -> (u32, u32, u32, u32)` — `(x, y, cw, ch)` with `cw = (w / 4) & !1`, `ch = cw * h / w` rounded even, `x = w - cw - w/40`, `y = h - ch - w/40`.
  - `pub fn over(base: &mut [u8], top_premul: &[u8])` — skips rows whose alpha is all zero.
  - `pub fn blend(from: &[u8], to: &[u8], t: f32, out: &mut [u8])` — `out = from*(1-t) + to*t`, alpha 255.
  - `pub const FADE: Duration = Duration::from_millis(500)`.

- [ ] **Step 1: Write the failing tests** (8×4 or 40×20 pictures of solid colours):
  - `camera_fills_and_words_go_over_it`: camera red, words with one opaque white row → that row white, others red.
  - `a_missing_camera_shows_the_room`: `camera: None`, scene `CameraWords` → room's pixels (words still over).
  - `the_corner_is_a_quarter_wide_bottom_right`: `corner_rect((1920,1080)) == (1392, 762, 480, 270)`; composed 40×20 ScreenCamera has camera colour at the corner's centre and room colour at (0,0).
  - `cards_go_over_black`: card None → all `[0,0,0,255]`.
  - `inputs_of_the_wrong_size_are_left_out`: camera slice 3 bytes short → treated as None, no panic.
  - `half_way_is_half_of_each`: blend `[200,0,0,255]`→`[0,100,0,255]` at 0.5 → `[100,50,0,255]`.
- [ ] **Step 2: Run** `... -- stream::compose` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `stream: put the stream picture together in the app`.

### Task 3: The pictures worker (scene, words, cards)

**Files:**
- Create: `src/stream/pictures.rs` (takes over `lyrics.rs`'s worker; `lyrics.rs` keeps `lower_third`, `key_for`, `Source`, `Words` and loses `input_args`, `Feed`, `start`, `run`)
- Modify: `src/stream/mod.rs`, `src/stream/lyrics.rs`, `src/record/mod.rs` (`RecordPrefs` fields below)

**Interfaces:**
- Consumes: Task 1 `choose`, `effective`, `LiveInfo`, `Choice`, `AutoPrefs`; `lyrics::key_for`; `render::cache::draw_key`.
- Produces:
  - In `RecordPrefs` (`#[serde(default)]` struct already): `pub auto_scenes: Option<bool>` (None = on), `pub presentation_camera: bool`, `pub cards: Cards` with `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)] pub struct Cards { pub starting: Card, pub ending: Card, pub hold: Card }`, `pub struct Card { pub words: String /* empty: the default text */, pub picture: String /* file path; empty: words */ }`. Remove `camera_layout` (old key ignored on load since unknown fields are ignored).
  - `pub fn default_words(scene: Scene) -> String` (tr: "Starting soon", "Thanks for joining us", "We'll be right back").
  - `pub type Info = Arc<dyn Fn() -> (LiveInfo, Choice, Option<String> /* countdown text */) + Send + Sync>` and `#[derive(Clone)] pub struct Look { pub info: Info, pub words: lyrics::Source, pub auto: AutoPrefs, pub cards: Cards, pub theme: Theme }` (`Debug` by hand like `lyrics::Words`).
  - `#[derive(Default)] pub struct Shown { pub scene: Scene /* default Screen */, pub auto: bool, pub words: Option<Arc<Vec<u8>>>, pub card: Option<Arc<Vec<u8>>> }` behind `pub type Slot = Arc<Mutex<Shown>>`.
  - `pub fn card_key(card: &Card, scene: Scene, countdown: Option<&str>, theme: &Theme, canvas: (u32,u32)) -> RenderKey` — text = words or default, `footer` = countdown for StartingSoon.
  - `pub fn start(look: Look, size: (u32,u32), camera: Arc<AtomicBool> /* seeing */, hold: Arc<AtomicBool>) -> Result<Worker, String>`; `Worker { pub slot: Slot }`, `stop()`, `Drop` stops. Every 100 ms: `choose` (with last scene), `effective` with `camera`, redraw words when the key changes, redraw the card when (scene, card, countdown) changes; card picture: decode `picture` with the `image` crate fitted to `size` (`record::pump::fit`), premultiplied; decode error → words card. While `hold` is set nothing changes.
- [ ] **Step 1: Write the failing tests:**
  - `the_starting_card_has_the_countdown_under_it`: `card_key(&Card::default(), StartingSoon, Some("4:32"), …)` → text "Starting soon", footer `Some("4:32")`; Ending → "Thanks for joining us", footer None.
  - `own_words_replace_the_default`: words "Welcome!" → text "Welcome!".
  - `the_worker_follows_what_is_live`: start with an `info` closure over a `Mutex<LiveInfo>`; Words slide → within 1 s `slot.scene == CameraWords` and `words.is_some()`; then `LiveInfo::default()` → `StartingSoon` and `card.is_some()`.
  - `a_card_picture_that_cannot_load_shows_its_words`: picture "C:/nope.png" → card drawn (Some), equal to the words card's bytes.
  - `freeze_keeps_the_frame_even_mid_fade`: hold set → changing info leaves `slot.scene` unchanged for 300 ms.
- [ ] **Step 2: Run** `... -- stream::pictures` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `... -- stream::` — PASS (lyrics tests updated: drop the `input_args` assert).
- [ ] **Step 5: Commit** `stream: one worker picks the scene and draws the words and cards`.

### Task 4: The camera in memory

**Files:**
- Modify: `src/stream/camera_feed.rs`

**Interfaces:**
- Produces: `pub fn camera_args(input: &[String], (w,h): (u32,u32), fps: u32) -> Vec<String>` now ends `format=rgba -f rawvideo -`, with black padding; `frame_bytes(size) = w*h*4`; `pub type Latest = Arc<Mutex<Option<(Arc<Vec<u8>>, Instant)>>>`; `Feed { pub latest: Latest, pub seeing: Arc<AtomicBool> }` (no `port`); `pub fn start(ffmpeg, input, size, fps) -> Result<Feed, String>`; `Feed::picture(&self) -> Option<Arc<Vec<u8>>>` (None when older than `STALE`). Drop `clear_frame`, `input_args`, the TCP sender and `hold` (Freeze is the writer's now). The worker loop keeps restarting the camera's FFmpeg every `RETRY` and sets `seeing` from freshness.
- [ ] **Step 1: Change the test** to `the_feed_is_rgba_with_black_bars`: `frame_bytes((4,2)) == 32`; `camera_args(...)` contains `-i video=Cam -an -vf scale=1280:720` and ends `format=rgba -f rawvideo -`. Add `the_camera_slot_goes_empty_when_pictures_stop`: a `Latest` holding a picture stamped `Instant::now() - 1s` → `picture()` None, `seeing` false after one loop (test the pure `fresh(latest, now) -> Option<Arc<Vec<u8>>>` helper).
- [ ] **Step 2: Run** `... -- stream::camera_feed` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** (together with Task 5 if the crate does not build in between; see Task 5).

### Task 5: The recorder composes

**Files:**
- Modify: `src/record/args.rs` (remove `Layout`, `Camera`, `picture_in_picture`, `camera_with_words`, `add_camera`, `Record::camera`, their tests), `src/record/run.rs` (`Plan`, `write`), `src/record/run_tests.rs`, `src/ui/operator/record.rs` (`record_or_stop` builds the new `Plan` fields)

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces:
  - `Plan { …, pub camera: Option<Vec<String>> /* FFmpeg input args */, pub look: Option<stream::pictures::Look> }` replacing `camera` and `words`.
  - `Recording::scene(&self) -> Option<(Scene, bool /* auto */)>` read from the worker's slot, for the buttons.
  - In `write`: before the loop start the camera feed (if `camera`) and the pictures worker (if `look`, with the feed's `seeing` or a never-true flag). Per tick: take the room picture as today (Freeze hands it back); if live with a look and not frozen: read `Shown` and the camera picture, `compose` into `next`; on a scene change start a fade (keep the previous output as `from`, `blend` by elapsed / `FADE`); write `out`. Compose only when the room, camera picture `Arc` pointer, words/card `Arc` pointers, scene or fade changed; otherwise write the previous output again. Not live (recording only): unchanged behaviour, the room as is.
- [ ] **Step 1: Change the real-FFmpeg tests** in `run_tests.rs`: `go_live_puts_the_camera_and_the_screen_together` and `go_live_puts_the_words_over_the_camera` now build `Plan { camera: Some(lavfi testsrc input args), look: Some(Look { info: fixed LiveInfo with Words slide, … }) }` and check the file's middle frame is camera-coloured with words in the lower third (keep their existing pixel-reading helper). Add `go_live_switches_scene_while_live`: info closure over a `Mutex<LiveInfo>`; Words for 2 s, then `slide: Picture` → frame at 1 s camera colour, frame at 4 s room colour.
- [ ] **Step 2: Run** `cargo test --profile quick --offline --lib -- record::run_tests` — FAIL (doesn't build).
- [ ] **Step 3: Implement** args, run, operator `record_or_stop` (look's `info` reads `app.stream_info()`, Task 6; until Task 6 lands use a closure returning `(LiveInfo from live, Choice::default(), None)`).
- [ ] **Step 4: Run** `... -- record::` and `... -- stream::` — PASS.
- [ ] **Step 5: Commit** `stream: the recorder puts camera, words and cards together; FFmpeg gets one picture`.

### Task 6: What is live, and the manual choice, on App

**Files:**
- Create: `src/stream/live_info.rs` (App methods, `impl App` in this file like `automation/actions.rs` does)
- Modify: `src/app.rs` (field `pub stream_choice: Mutex<stream::scene::Choice>`, initialised `Default`), `src/stream/mod.rs`

**Interfaces:**
- Produces:
  - `App::stream_info(&self) -> (LiveInfo, Choice, Option<String>)` — from `live` (cursor, `frame.media`/`media_status` Playing → `media_playing`, `frame.slide` → `SlideKind`), `last_item` = cursor's `item_index + 1 ==` the live service's item count, `pre_service` = `automation.pre.is_some()`, countdown = the pre-service timer's `TimerSnapshot` formatted with the timers module's shared formatter when `pre_service`.
  - `App::set_stream_scene(&self, scene: Option<Scene>)` — `Choice::pick(scene, cursor item)`.
  - `App::clear_stream_choice(&self)` — called at Go Live start.
- [ ] **Step 1: Failing tests** (use the test app helper other `app` tests use): `stream_info_reads_the_live_slide` (show a song item → `slide: Words`, cursor Some); `a_manual_scene_is_kept_for_the_live_item` (set Camera → `stream_info().1 == Choice { scene: Some(Camera), item: Some(id) }`); `the_last_item_is_known` (show the last item, clear the slide → `last_item: true`, `slide: None`).
- [ ] **Step 2–4:** run `... -- stream::live_info`, implement, pass; switch Task 5's operator closure to `app.stream_info()`; call `clear_stream_choice` in `record_or_stop` when `live`.
- [ ] **Step 5: Commit** `stream: what is live and the operator's choice, for the scenes`.

### Task 7: Scene buttons while live

**Files:**
- Modify: `ui/operator/topbar.slint` (beside Freeze/Mute, lines ~610–640), `ui/operator.slint` (pass-through properties/callback near line 284 and 944), `src/ui/operator/record.rs` (ticker and wire), `src/ui/operator/record_tests.rs`

**Interfaces:**
- Produces: Slint `in property <[string]> scene-labels; in property <int> scene-lit /* -1 = Auto */; in property <int> scene-out; callback pick-scene(int /* -1 = Auto */);`. Rust: `Operator::pick_scene(&self, i: i32)` → `app.set_stream_scene(Scene::ALL.get(i))`; the 1 s ticker (`show_recording`) also sets `scene-out` from `Recording::scene()` and `scene-lit` from `lit`. Labels set at Go Live from `Scene::label`. Buttons: "Auto" then the seven, same widget as Freeze/Mute, `on:` when lit (Auto lit while `scene-lit == -1`); the scene going out gets the live red outline. Hidden when not live. The ticker runs every 1 s; make the scene part run on a 250 ms timer so a click lights within a quarter second.
- [ ] **Step 1: Failing test** in `record_tests.rs`: `a_scene_click_lights_and_holds_until_the_next_item` — with the existing go-live test setup, `op.pick_scene(0)` → `app.stream_choice` is Camera on the live item; show another item → `lit` None.
- [ ] **Step 2–4:** run `... -- ui::operator::record`, implement, pass.
- [ ] **Step 5: Commit** `ui: scene buttons beside Go Live`.

### Task 8: Settings and the macro step

**Files:**
- Modify: `ui/dialogs/settings.slint` + `settings-state.slint` (Livestream section: remove the camera-layout picker; add Automatic scenes checkbox, Presentations combo, three card rows each with a words field and Choose picture… / Remove picture), `src/ui/dialogs/settings_record.rs` (fill/save), `src/automation/macros.rs` (`MacroStep::StreamScene { scene: Option<Scene> }` → `PlannedStep::StreamScene`, class Action, serde `{"step":"stream_scene","scene":"camera"}` / `null` for Auto), `src/automation/actions.rs` (`stage_step`: `StreamScene { scene } => self.set_stream_scene(scene)`), `src/automation/slide_actions.rs` (palette tiles: one per scene and "Stream: automatic", group Stage or a new `Group::Stream` if groups are listed in the UI), `src/ui/dialogs/automation.rs` (step label: "Stream scene: {name}" / "Stream scene: automatic"), `translations/*` (run `cargo run --example i18n_extract`).
- [ ] **Step 1: Failing tests:** `macros.rs` `stream_scene_step_round_trips` (json above both ways; unknown scene name loads as `Lenient::Unknown`, as other steps do); `actions_tests.rs` `a_slide_action_can_pick_the_stream_scene` (slide action with StreamScene Camera, show that slide → `app.stream_choice.lock().scene == Some(Camera)`); `settings_tests.rs` `livestream_scene_settings_save` (set checkbox off and card words → saved `RecordPrefs.auto_scenes == Some(false)`, `cards.hold.words == "Back soon"`).
- [ ] **Step 2–4:** run `... -- automation::` and `... -- ui::dialogs::settings`, implement, pass; run `cargo run --example i18n_extract` then `cargo test --profile quick --offline --test i18n_catalogs`.
- [ ] **Step 5: Commit** `stream: scene settings, and a Stream scene step for slide actions and macros`.

### Task 9: Whole check and a build to try

- [ ] Run `quick-check.cmd stream` and `quick-check.cmd record` (`< NUL`) — PASS.
- [ ] `try-latest.cmd follow-sunday < NUL` in the background; the build is done when `rustypresenter.exe` appears running (it does not exit until the app closes). Then `make-portable.cmd < NUL`.
- [ ] Update `PROJECT_PLAN.md` handoff log with what was built and not verified (real camera, real stream).
