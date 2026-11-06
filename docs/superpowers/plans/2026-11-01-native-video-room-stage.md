# Native App — Plan 2: Video, Room Output, Stage

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A native `rustypresenter-native` binary that runs the existing `App` core and phone-remote server, and shows the room output and stage screens natively with Slint: slides from the Rust compositor, video and audio through libmpv drawn under the Slint scene, crossfades, borderless fullscreen on the chosen monitor, and media reports back into the live engine. The operator window is Plan 3; until then the native binary serves today's web operator page on `http://127.0.0.1:<port>/` so the room and stage can be driven and tested.

**Architecture:** New modules `video` (libmpv binding + players + GL/software drawing), `room` (pure Rust: decides what each layer shows and when a frame is committed), `platform` (monitors, fullscreen), and `ui` (`.slint` files + glue for room and stage). The room glue subscribes to `App::subscribe()`, asks `room::RoomState` what to change, prepares images off the UI thread, swaps on the Slint event loop, and reports `RoomEvent`s through `App::room_event`. The Tauri app keeps building and working.

**Tech Stack:** Rust 2024, Slint 1.18 (`backend-winit`, `renderer-femtovg`, `renderer-software`, `unstable-winit-030`), libmpv client API 2.x loaded with `libloading` 0.8, `glow` 0.18 for GL state save/restore, existing `render` module.

**Spec:** `docs/superpowers/specs/2026-10-29-native-slint-design.md` §3, §5, §6, §7, §8.1–8.2. **Check results:** `docs/superpowers/checks/2026-native-checks.md` (C1 gotchas 1–7 are requirements for Tasks 1–4).

## Global Constraints

- All Plan 1 constraints still apply (Tauri app builds and `cargo test` passes after every task; no blocking work on a UI thread; libmpv LGPL, loaded at runtime, never linked).
- libmpv lookup order: env `RP_LIBMPV` (full path) → next to the executable → `<data dir>/deps/mpv/` → system names (`libmpv-2.dll`/`mpv-2.dll` on Windows, `libmpv.2.dylib`/`libmpv.dylib` on macOS, `libmpv.so.2`/`libmpv.so` on Linux). Missing libmpv is not an error at startup: video/audio items report `PlayState::Error` with "The video player is missing. Reinstall RustyPresenter." and everything else works.
- mpv rendering: render context created only inside `RenderingSetup`; `loadfile` only after the render context exists; `MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME = 0`; FBO = the one bound at `BeforeRendering`, size `window.size()` physical, `flip_y = 1`; draw every Slint frame while a video layer is active; save/restore the GL state list from C1 gotcha 3; free the render context in `RenderingTeardown` before `mpv_terminate_destroy`; the update callback never calls mpv, it only coalesces (AtomicBool) and posts `request_redraw` via `invoke_from_event_loop`.
- Software fallback (`RP_FORCE_SOFTWARE=1` or GL failure): Slint `renderer-software` + `MPV_RENDER_API_TYPE_SW`, format `rgb0`, 64-byte-aligned stride, width rounded down to a multiple of 16, shown as an `Image` under the scene.
- Never compare exact pixel hashes to count video frames (dither noise ±1/255).
- Timeouts for preparing a layer (same as `web/output.js`): slide/still/logo 2.5 s, background video 6 s, foreground video and audio 8 s. A timeout or load error reports `RoomEvent::Failed { version, reason }` and keeps the committed output.
- Crossfade 400 ms (CSS `.layer` transition today). A change that starts or stops a foreground video fades through black.
- Mpv `time-pos` reports at most every 500 ms (as the web room does), plus every state change.
- Each `.slint` file and glue file covers one screen; keep each under ~400 lines.

## Review Focus

1. **Stale media reports** — a video from a retired instance still sending `time-pos` after the operator moved on. Expected: reports carry the instance id and `Live::report` ignores them (existing rule); the player for a retired instance is stopped within one event-loop turn. Test in Task 2/5.
2. **Prepare fails** — a missing asset file or a corrupt video. Expected: `Failed` reported with a readable reason, committed layers stay on screen. Test in Task 4.
3. **Rapid Next** — five versions published while the first is still preparing. Expected: only the newest is applied and committed; earlier prepared layers are disposed (the `token` rule in `output.js`). Test in Task 4.
4. **Monitor unplugged** — room monitor disappears. Expected: room window closes, operator notice, reopens on the same monitor id when it returns, never on the operator's monitor. Test in Task 6 (logic) + manual.
5. **No libmpv** — expected: app starts, slides work, video items report the missing-player error. Test in Task 1/2.
6. **GL state leakage** — Slint text corrupt after mpv draws. Expected: none, verified by the C1-style readback check in Task 3.

---

### Task 1: libmpv binding and discovery (`video::ffi`)

**Files:**
- Create: `src/video/mod.rs`, `src/video/ffi.rs`
- Modify: `src/lib.rs`, `Cargo.toml` (`libloading = "0.8"`)

**Interfaces:**
- Produces:
  - `pub struct Mpv` (the loaded library; `Send + Sync`), `pub fn Mpv::load() -> Result<Arc<Mpv>, String>` using the lookup order above; `pub fn Mpv::load_from(path: &Path) -> Result<Arc<Mpv>, String>`; `pub fn Mpv::candidates(data_dir: &Path) -> Vec<PathBuf>`.
  - `pub fn api_version(&self) -> (u32, u32)`; requires major 2, else error "This libmpv is too old (client API x.y); RustyPresenter needs 2.x."
  - Raw functions from the C1 spike `mpv.rs` plus `observe_property`, `set_property` (string), `command_async`, `get_property` (double/flag), `wakeup` callbacks not needed (event thread uses `wait_event`).
  - `pub const MISSING: &str = "The video player is missing. Reinstall RustyPresenter.";`

- [ ] **Step 1: Write the failing tests** in `ffi.rs`: `candidates_start_with_env_override` (set `RP_LIBMPV`, first candidate equals it), `candidates_include_exe_dir_and_data_dir`, `load_from_missing_file_is_a_readable_error` (message contains the path), and an env-gated `#[ignore]`-free test `loads_real_libmpv_when_available` that returns early unless `RP_VIDEO_TESTS=1` and then asserts `api_version().0 == 2`.
- [ ] **Step 2:** `cargo test video::` → FAIL.
- [ ] **Step 3:** Implement by moving the spike binding into `ffi.rs` (keep its constants, add the missing ones from `client.h`: `MPV_FORMAT_*`, `EVENT_PROPERTY_CHANGE`=22, `EVENT_PLAYBACK_RESTART`=21, `EVENT_FILE_LOADED`=8, `EVENT_START_FILE`=6; `mpv_event_end_file { reason, error, playlist_entry_id, ... }`, `mpv_event_property { name, format, data }`).
- [ ] **Step 4:** `cargo test` passes; with `RP_VIDEO_TESTS=1 RP_LIBMPV=<path>` the real-library test passes on Windows.
- [ ] **Step 5:** Commit `feat(video): runtime libmpv binding and discovery`.

### Task 2: Players and events (`video::player`)

**Files:**
- Create: `src/video/player.rs`
- Modify: `src/video/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum Role { Background, Foreground, Audio }` — Background: `mute=yes`, `loop-file=inf`; Foreground: sound, `loop-file` per `EndAction::Loop`, `keep-open=yes` for `EndAction::Hold`; Audio: `vid=no`, `loop-file` per `looped`.
  - `pub struct PlayerReport { pub inst: u64, pub state: PlayState, pub position: f64, pub duration: f64, pub error: Option<String> }` and `impl From<PlayerReport> for RoomEvent`.
  - `pub struct Player` — `Player::new(mpv: Arc<Mpv>, role: Role, inst: u64, reports: std::sync::mpsc::Sender<PlayerReport>) -> Result<Player, String>`; `load(&self, path: &Path, start: f64)`; `set_paused(&self, bool)`; `position(&self) -> f64`; `handle(&self) -> *mut mpv_handle` (for Task 3); `Drop` stops the event thread and calls `mpv_terminate_destroy` (only after any render context is freed — `Player` owns an `Option<RenderContext>` slot that Task 3 fills and drops first).
  - Common options: `hwdec=auto-safe`, `vo=libmpv`, `audio-device` from settings (empty = auto), `idle=yes`, `input-default-bindings=no`, `osc=no`, `terminal=no`.
  - Event thread: observes `pause`, `time-pos`, `duration`, `eof-reached`; maps `playback-restart`/unpause → Playing, `pause=yes` → Paused, `end-file` reason EOF → Ended, reason ERROR → Error with `mpv_error_string`; `time-pos` throttled to ≥ 500 ms between reports.
  - Missing libmpv: `Player::new` with no library is not possible; callers use `video::players(mpv: Option<Arc<Mpv>>)` which, on `None`, immediately reports `Error` with `ffi::MISSING` for the instance.

- [ ] **Step 1: Write the failing tests** (env-gated on `RP_VIDEO_TESTS=1`, so CI stays green without libmpv; each uses lavfi sources, no sample files): `audio_sine_plays_then_ends` (`av://lavfi:sine=frequency=440:duration=1`, Role::Audio: Playing then Ended within 5 s, every report has the given inst), `video_reports_duration_and_position` (`av://lavfi:testsrc2=size=320x240:rate=30:duration=2`, Role::Foreground with no render context uses `vo=null` in tests), `pause_reports_paused`, `missing_file_reports_error_with_reason`, `loop_never_ends` (Role::Background, 1.5 s source, no Ended within 4 s), `drop_stops_reports` (no reports after drop). Non-gated: `missing_libmpv_reports_error_for_instance`, `report_converts_to_room_event`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement. `vo` is set by the caller: `libmpv` when a render context will be attached, `null` for probes/tests.
- [ ] **Step 4:** `cargo test` passes; gated tests pass on Windows with libmpv.
- [ ] **Step 5:** Commit `feat(video): mpv players reporting playback by instance`.

### Task 3: Drawing video in a Slint window (`video::surface`)

**Files:**
- Create: `src/video/surface.rs`, `examples/video_surface.rs`
- Modify: `Cargo.toml` (`slint = { version = "1.18", features = ["unstable-winit-030"] }`, `glow = "0.18"`), `src/video/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum DrawMode { OpenGl, Software }`, `pub fn choose_backend() -> DrawMode` (reads `RP_FORCE_SOFTWARE`; sets `BackendSelector` winit + femtovg or software; falls back to Software if the GL selector fails).
  - `pub struct VideoSurface` — `VideoSurface::attach(window: &slint::Window, mode: DrawMode, on_frame: impl Fn(Option<slint::Image>) + 'static) -> Rc<VideoSurface>`; `set_player(&self, player: Option<Rc<Player>>)` (only one drawn at a time: the foreground video if any, else the background video); `add(&self, player: Rc<Player>)` (gives a player its own render context undrawn, so it loads and reports `Playing` while being prepared; a context that cannot be made is reported as its `Error`); `remove(&self, player: &Rc<Player>)` (frees it when the player is disposed; players stay added until then); `active(&self) -> bool`.
  - OpenGL path: rendering notifier implementing the C1 rules (create render context per attached player inside `RenderingSetup` or, when the player arrives later, on the next `BeforeRendering` with the notifier's `get_proc_address`; the deferred `loadfile` is sent after creation); GL state save/restore exactly as the spike's list; teardown order.
  - Software path: `MPV_RENDER_API_TYPE_SW` render on each update into an aligned buffer → `SharedPixelBuffer<Rgb8Pixel>` → `on_frame(Some(image))` on the event loop.
  - `examples/video_surface.rs`: the C1 check rebuilt on `VideoSurface` (transparent window, white "Amazing grace" over `testsrc2`), with `--sw` and a `--measure` flag printing distinct frames/s and text-pixel stability like the spike.

- [ ] **Step 1:** Port the spike's `main.rs` drawing code into `surface.rs` behind the interface; keep the readback measurement only in the example.
- [ ] **Step 2:** `cargo run --release --example video_surface -- --measure` and `-- --sw --measure` on Windows: ≥ 28.5 distinct frames/s for 1080p30 testsrc2 in both modes, text pixels stable in every sample (same thresholds as C1).
- [ ] **Step 3:** `cargo test` passes (no new unit tests; the example is the check).
- [ ] **Step 4:** Record numbers in `2026-native-checks.md` under a new row "C1 via `video::surface`". Commit `feat(video): draw mpv under Slint with OpenGL or software`.

### Task 4: Room decisions (`room`)

**Files:**
- Create: `src/room.rs`
- Modify: `src/lib.rs`, `src/app.rs` (`room_event` becomes `pub`)

**Interfaces:**
- Produces (pure Rust, no Slint):
  - `#[derive(Clone, PartialEq, Eq, Hash, Debug)] pub enum LayerKey { Background(u64), Media(u64), Slide(String), Logo(String), Audio(u64) }` — slide key = JSON of the slide layer + theme JSON, logo key = asset id (mirrors `keys()` in `output.js`).
  - `pub enum Kind { Background, Media, Slide, Logo, Audio }`, `pub struct Want { pub kind: Kind, pub key: LayerKey, pub timeout: Duration }`.
  - `pub struct RoomState` with `new()`, `plan(&mut self, view: &LiveView) -> Option<Plan>` where `Plan { pub token: u64, pub version: u64, pub prepare: Vec<Want>, pub remove: Vec<Kind>, pub fade_through_black: bool }` — `None` when version and theme/logo are unchanged (pause sync only, see `pause_sync`).
  - `fn ready(&mut self, token: u64) -> Ready` → `Ready::Swap { commit: RoomEvent }` if `token` is still the newest, else `Ready::Stale` (caller disposes what it prepared).
  - `fn failed(&mut self, token: u64, reason: String) -> Option<RoomEvent>` → `Failed` only for the newest token.
  - `pub fn pause_sync(view: &LiveView) -> Vec<(u64, bool)>` (instance, paused) for media and audio.
  - `pub fn draw_background(frame: &Frame) -> bool` — false while a foreground **video** is live (spec §5).

- [ ] **Step 1: Write the failing tests:** `first_view_prepares_every_present_layer`, `same_version_same_theme_is_no_plan`, `theme_change_reprepares_only_slide`, `new_media_instance_reprepares_media_only`, `clear_slide_removes_slide_without_preparing`, `rapid_versions_only_newest_commits` (plan v1, plan v2, `ready(token1)` → Stale, `ready(token2)` → Swap with `Committed{version:2}`), `failed_old_token_is_silent`, `failed_newest_reports_failed_with_reason`, `video_start_fades_through_black`, `still_does_not_fade_through_black`, `background_hidden_under_foreground_video`, `timeouts_match_web_room`, `pause_sync_lists_media_and_audio`.
- [ ] **Step 2:** `cargo test room::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(room): decide layer changes and commits like the web room`.

### Task 5: Room window (`ui/room.slint`, `ui::room`)

**Files:**
- Create: `ui/room.slint`, `ui/theme.slint`, `src/ui/mod.rs`, `src/ui/room.rs`, `build.rs` changes (`slint-build` compiling `ui/*.slint`; keep `tauri_build::build()`)
- Modify: `Cargo.toml` (`slint-build = "1.18"` build-dep; `i-slint-backend-testing = "1.18"` dev-dep)

**Interfaces:**
- `room.slint`: `export component RoomWindow inherits Window` — `background: transparent` over black when no video; properties `in property <image> bg-image`, `in property <color> bg-color`, `in property <bool> bg-visible`, `in property <image> media-still`, `in property <image> slide`, `in property <image> logo`, `in property <image> video-frame` (software mode), `in property <float> black` (0–1 fade-through-black overlay), each image layer with an `opacity` animated 400 ms; `mouse-cursor: none`; element ids for tests (`RoomWindow::slide` etc.).
- `src/ui/room.rs`: `pub struct Room` — `Room::open(app: Arc<App>, renderer: Arc<Renderer>, mpv: Option<Arc<Mpv>>, monitor: Option<MonitorId>) -> Result<Rc<Room>, String>`; subscribes to updates (a tokio task forwarding `Update::Live` into `invoke_from_event_loop`), runs `RoomState::plan`, prepares each `Want` off the UI thread (slides via `Renderer::render` at the window's physical size; images decoded with `image` at display size; videos/audio by creating `Player`s and waiting for the first `Playing` or `FileLoaded` with the timeout), then on `Ready::Swap` sets properties, swaps players into the `VideoSurface`, applies `pause_sync`, and calls `app.room_event(commit)`. Player reports go to `app.room_event(report.into())`. Dropping a replaced layer disposes its player after the 400 ms fade.
- Move `image` from dev-dependencies to dependencies (`png`, `jpeg`, `gif`, `webp`, `bmp` features).

- [ ] **Step 1: Write the failing tests** in `src/ui/room.rs` using the testing backend (`i_slint_backend_testing::init_integration_test_with_mock_time()` or `init_no_event_loop()`; build with `SLINT_EMIT_DEBUG_INFO=1` per C4): `text_slide_sets_slide_image_and_commits` (test `App` over a temp store, take a scripture text live, pump the event loop until the `slide` property has a non-zero size; `live_view().committed == version`), `clear_all_hides_layers_and_commits`, `missing_still_asset_reports_failed_and_keeps_previous`, `logo_layer_follows_frame_logo`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes (the testing-backend tests set `SLINT_EMIT_DEBUG_INFO=1` via `.cargo/config.toml` `[env]`).
- [ ] **Step 5:** Commit `feat(ui): native room output window`.

### Task 6: Monitors and fullscreen (`platform::monitors`)

**Files:**
- Create: `src/platform/mod.rs`, `src/platform/monitors.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- `#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)] pub struct MonitorId { pub name: String, pub size: (u32, u32), pub position: (i32, i32) }`; `MonitorId::matches_saved(&self, saved: &str) -> bool` — a saved Windows device name (`\\.\DISPLAY2`, today's `DisplayRoles`) matches by name; a saved `name|w×h|x,y` string matches all three; a saved string with the same name but a different size still matches by name when no exact match exists (`pick`).
- `pub fn pick(monitors: &[MonitorId], saved: Option<&str>, operator: Option<&MonitorId>) -> Option<MonitorId>` — exact match first, then name-only; never returns the operator's monitor unless it was explicitly saved.
- `pub fn list(window: &slint::Window) -> Vec<MonitorId>` (winit `available_monitors`); `pub fn fullscreen_on(window: &slint::Window, id: &MonitorId) -> Result<(), String>` (`Fullscreen::Borderless(Some(m))`).
- `pub struct Watcher` — every 2 s compares `list()`; emits `Gone(role)` / `Back(role)` for room and stage; the native `Screens` impl (Task 8) closes and reopens windows and sets `live.notice`.
- Converting `Vec<MonitorId>` to the existing `displays::Monitor` JSON shape so the web operator's Screens dialog keeps working.

- [ ] **Step 1: Write the failing tests:** `windows_device_name_matches_saved_choice`, `exact_triple_beats_name_only`, `name_only_used_when_resolution_changed`, `never_picks_operator_monitor_implicitly`, `explicitly_saved_operator_monitor_is_allowed`, `watcher_reports_gone_and_back` (feed lists to a pure `Watcher::step`).
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(platform): monitor ids, choice and unplug watch`.

### Task 7: Stage window (`ui/stage.slint`, `ui::stage`)

**Files:**
- Create: `ui/stage.slint`, `src/ui/stage.rs`

**Interfaces:**
- Same content as `web/stage.html`: connection dot, item title ("Nothing live" / "Waiting for the service…"), clock (h:mm, updated each second by a Slint `Timer`), remaining media time `−m:ss` in red, current slide text fitted (largest size ≤ 9% of height down to 3% in 0.4% steps until it fits), dimmed to 35% when nothing is on air, notes box when the slide has notes, Next box (amber) with the next slide text or "End of service". Non-text slides show "(slide picture)", "(video)", "(picture)", "(audio)".
- `pub struct Stage` — `Stage::open(app: Arc<App>, monitor: Option<MonitorId>) -> Result<Rc<Stage>, String>`; pure helper `pub fn stage_text(view: &LiveView) -> StageText { title, now, now_dim, notes, next, timer }` unit-tested without Slint.

- [ ] **Step 1: Write the failing tests:** `stage_text_for_lyric_with_next`, `stage_text_end_of_service`, `stage_text_dims_when_cleared`, `stage_text_timer_counts_down`, `stage_text_labels_non_text_slides`, plus one testing-backend test that the `now` text element shows the live lyric.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** pass.
- [ ] **Step 5:** Commit `feat(ui): native stage window`.

### Task 8: Native binary (`rustypresenter-native`)

**Files:**
- Create: `src/bin/native.rs`, `src/ui/screens.rs`
- Modify: `Cargo.toml` (`[[bin]] name = "rustypresenter-native"`; keep the Tauri binary the default `run` target), `dev-build.cmd` (also builds the native binary), `README.md` (how to try it)

**Interfaces:**
- Starts the same data folder, `App`, and phone/web server as `main.rs` (shared startup moved to `pub fn rustypresenter::start(...)` if needed; Tauri `main.rs` calls it too). Port and single-instance behavior as today. A second binary running beside the Tauri app reports "RustyPresenter is already running" and exits.
- Runs the Slint event loop on the main thread and tokio on a background runtime.
- `NativeScreens: Screens` opens/closes `Room` and `Stage` windows on the Slint thread via `invoke_from_event_loop` and a oneshot reply; reopens them from `settings.displays` at start; implements unplug behavior from Task 6.
- Until Plan 3: prints and opens (system browser, once) `http://127.0.0.1:<port>/` for the operator page; `--no-browser` disables that.
- Panics are logged to `<data>/logs/native-YYYYMMDD.log` with a backtrace.

- [ ] **Step 1:** Implement.
- [ ] **Step 2:** `cargo build --release --bins` and `cargo test` pass.
- [ ] **Step 3: Manual check on Windows** (record results in `2026-native-checks.md`): open room on the one monitor; take a lyric, a scripture with reference, a lower-third song, a still, a background video, a foreground video with sound, an audio item; Next/Back; Clear All/Slide/Media/Audio/Logo; pause from the web operator; stage window shows title/now/next/timer; phone remote drives the native room. Unplug test waits for the church laptop.
- [ ] **Step 4:** Commit `feat: native binary with room and stage windows`.

### Task 9: CI and records

**Files:**
- Modify: `.github/workflows/ci.yml` (Linux: add `libxkbcommon-dev libwayland-dev libgl1-mesa-dev` if the Slint build needs them; build `--bins`), `PROJECT_PLAN.md` (NATIVE-P2 status, handoff), `docs/superpowers/checks/2026-native-checks.md`

- [ ] **Step 1:** Push the branch; CI green on all three OSes (gated video tests skipped there).
- [ ] **Step 2:** Update the plan and checks doc. Commit `docs: Plan 2 results`.

---

**After this plan:** write Plan 3 (operator window and dialogs) using the room/stage glue patterns from Tasks 5 and 7.
