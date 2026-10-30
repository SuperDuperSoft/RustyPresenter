# Native App — Plan 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prepare the codebase for the native Slint app: git + three-platform CI, the five feasibility checks (C1–C5), the `App` core freed from axum and Tauri, a typed update stream, and the cross-platform slide compositor.

**Architecture:** The existing Tauri app keeps working throughout. `App` and its action dispatcher move out of `server.rs` into `src/app.rs` with no axum or Tauri types, so a native UI can call them in-process later. Slide text rendering moves from `web/render.js` into a Rust `render` module (cosmic-text + tiny-skia) with golden-image tests. Feasibility checks run as throwaway crates under `experiments/` and record results in one doc.

**Tech Stack:** Rust 2024, tokio, axum 0.8 (phone remote), Slint 1.18 (checks only in this plan), cosmic-text, tiny-skia, libmpv (checks only), pdfium-render (checks only), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-29-native-slint-design.md`

Later plans (written after this one finishes, using the C1–C5 results): Plan 2 video + room output + stage; Plan 3 operator window and dialogs; Plan 4 imports, crash recovery, packaging, Tauri removal.

## Global Constraints

- Target platforms: Windows 10+, macOS 12+, current Linux desktop (X11 and Wayland).
- Slint is used under the Royalty-Free License v2.0; no Slint APIs exposed to third parties.
- libmpv must be an LGPL build (`-Dgpl=false`) and loaded at runtime, never linked at build time.
- The Tauri app must still build and pass `cargo test` at the end of every task.
- No blocking work on a UI thread; file I/O and rendering run off it.
- Compositor values (from `web/render.js`, copied exactly): logical canvas 1920 wide; padding 110 px sides, 66 px top/bottom (70 px bottom for lower third); lyrics weight 600, line height 1.22, centered; scripture weight 500, line height 1.32, left-aligned; reference 44 px weight 600 bottom-right; footer 30 px weight 400 bottom-center; footer/reference opacity 0.88, bottom padding 46 px; fit steps −4 px above 60 px else −2 px, floor 26 px; lower third gradient covers bottom 55%, transparent → rgba(0,0,0,0.55).
- Default bundled font: Noto Sans (OFL), regular 400, medium 500, semibold 600.

## Review Focus

1. **Theme font not installed** — the default theme font is "Segoe UI", which macOS and Linux don't have. Expected: the bundled Noto Sans is used and `Rendered::font_fallback` is true, no panic. Test in Task 6.
2. **Lyrics too long to fit at 26 px** — expected: drawn at 26 px, words never dropped, overflow clipped at the canvas edge. Test in Task 5.
3. **A single word wider than the text box** (a URL, a long German compound) — expected: it breaks across lines like CSS `overflow-wrap: break-word`, not overflowing sideways. Test in Task 5.
4. **Non-Latin text** (Spanish accents, Korean, emoji) — expected: glyphs render through system font fallback, no panic, no blank image. Test in Task 6.
5. **Bad theme color** (`"#zz"`, `""`, `"red"`) — expected: `"red"`-style CSS names and `#rgb`/`#rrggbb` parse; anything else falls back to white. Test in Task 6.

---

### Task 1: Git repository and three-platform CI

**Files:**
- Create: `.github/workflows/ci.yml`, `.gitattributes`
- Modify: `.gitignore` (add `/experiments/*/target/`)

**Interfaces:**
- Produces: a `main` branch; CI job `test` on `windows-latest`, `macos-latest`, `ubuntu-24.04` running `cargo test --locked`.

- [ ] **Step 1:** `git init -b main`; `.gitattributes` with `* text=auto eol=lf` and `*.cmd text eol=crlf`.
- [ ] **Step 2:** Write `ci.yml`: matrix over the three OSes; `dtolnay/rust-toolchain@stable`; `Swatinem/rust-cache@v2`; on Ubuntu install `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libmpv-dev libfontconfig-dev` (Tauri still present; mpv/fontconfig for later tasks); run `cargo test --locked`.
- [ ] **Step 3:** Run locally: `cargo test --locked` → all 27 existing tests pass.
- [ ] **Step 4:** Commit everything except ignored files: `git add -A && git commit -m "chore: start git history and CI for Windows, macOS, Linux"`.

### Task 2: Feasibility checks C1 and C2 (Slint + mpv, monitors)

Throwaway code. Result is a recorded answer, not kept code.

**Files:**
- Create: `experiments/c1-mpv-underlay/` (own `Cargo.toml`, `src/main.rs`), `experiments/c2-monitors/`, `docs/superpowers/checks/2026-native-checks.md`

**Interfaces:**
- Produces: `2026-native-checks.md` rows for C1, C2 with PASS/FAIL per OS, the exact Slint features and renderer used, and any workaround. Plan 2 reads this.

- [ ] **Step 1: C1.** Slint 1.18 window with renderer `renderer-femtovg` and backend `backend-winit`; `set_rendering_notifier`, on `RenderingSetup` create `mpv_render_context` with `MPV_RENDER_API_TYPE_OPENGL` using the provided `get_proc_address`; on `BeforeRendering` call `mpv_render_context_render` to the default framebuffer, restoring GL state; Slint scene: a transparent root with a white `Text` "Amazing grace" on top. libmpv loaded with `libloading`. Play `experiments/media/sample-1080p.mp4` (H.264, 30 s).
- [ ] **Step 2:** Run on Windows, macOS, Linux (X11 and Wayland). PASS = video under readable text, no tearing, frame rate within 5% of source (mpv `estimated-frame-number` vs elapsed time), CPU noted.
- [ ] **Step 3: C1 fallback.** Same window with `renderer-software` and `MPV_RENDER_API_TYPE_SW` into an RGBA buffer shown as a Slint `Image`. Record CPU % at 1080p30.
- [ ] **Step 4: C2.** Slint feature `unstable-winit-030`; via `slint::winit_030::WinitWindowAccessor::with_winit_window`, list `available_monitors()` (name, position, size, scale) and set `Fullscreen::Borderless(Some(monitor))` on the second monitor. PASS = window covers exactly the chosen monitor; on macOS it does not open a new Space.
- [ ] **Step 5:** Record results in `2026-native-checks.md` (table: check, OS, result, notes). Commit `experiments/` and the doc: `git commit -m "spike: C1 mpv under Slint, C2 monitors"`.

### Task 3: Feasibility checks C3, C4, C5 (file drop, UI testing, LibreOffice)

**Files:**
- Create: `experiments/c3-file-drop/`, `experiments/c4-ui-testing/`, `experiments/c5-libreoffice/`
- Modify: `docs/superpowers/checks/2026-native-checks.md`

**Interfaces:**
- Produces: C3–C5 rows in the checks doc.

- [ ] **Step 1: C3.** Slint window; check whether dropping files from the OS file manager produces an event with the file paths, either through Slint or through winit `WindowEvent::DroppedFile` via the winit accessor. PASS per OS = paths received.
- [ ] **Step 2: C4.** A small `.slint` component (button + text). Using Slint's testing backend (`i-slint-backend-testing`, `ElementHandle` queries), a `#[test]` clicks the button and asserts the text changed. PASS = runs headless in CI on all three OSes.
- [ ] **Step 3: C5.** Convert `experiments/media/sample.pptx` (3 slides) with `soffice --headless --convert-to pdf --outdir <tmp>`, then render each page with `pdfium-render` (pdfium loaded at runtime) to PNG 1920 px wide. PASS = 3 PNGs matching the slides, on each OS. Record LibreOffice paths found per OS.
- [ ] **Step 4:** Record results; commit `git commit -m "spike: C3 file drop, C4 UI testing, C5 LibreOffice import"`.

### Task 4: Move `App` and the dispatcher into `src/app.rs`

**Files:**
- Create: `src/app.rs`
- Modify: `src/server.rs` (keeps routes, SSE, pairing HTTP, static/asset serving), `src/lib.rs`, `src/main.rs`, `src/displays.rs`

**Interfaces:**
- Consumes: existing `App`, `DataPlan`, `api_inner` match arms.
- Produces:
  - `pub struct ActionError { pub status: u16, pub message: String }` with `ActionError::bad(msg) -> Self` (400) and `ActionError::forbidden(msg) -> Self` (403).
  - `impl App { pub async fn do_action(self: &Arc<Self>, action: &str, body: &[u8]) -> Result<serde_json::Value, ActionError> }` containing every arm now in `api_inner`, unchanged in behavior.
  - `pub trait Screens: Send + Sync { fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String>; fn close(&self, role: &str); }` and `App::set_screens(&self, s: Box<dyn Screens>)`. The Tauri window code in `displays.rs` implements it; `App` no longer holds `tauri::AppHandle` or `displays::Window`.
  - `App`, `DataPlan`, `Job`, `Phone`, `Pairing` live in `app.rs`; `server.rs` maps `ActionError` to an HTTP response with the same status and `{"error": message}` body as today.

- [ ] **Step 1: Write the failing tests** in `src/app.rs`:

```rust
#[tokio::test]
async fn do_action_saves_a_song_and_returns_its_id() {
    let app = test_app(); // App over Store::open(tempdir)
    let v = app.do_action("song/save", br#"{"id":"00000000-0000-0000-0000-000000000000","title":"Amazing Grace","lyrics":"Verse 1\nAmazing grace"}"#).await.unwrap();
    let id: Uuid = serde_json::from_value(v["id"].clone()).unwrap();
    assert!(!id.is_nil());
    assert_eq!(app.data.lock().unwrap().songs[0].title, "Amazing Grace");
}

#[tokio::test]
async fn do_action_rejects_untitled_song_with_400() {
    let err = test_app().do_action("song/save", br#"{"id":"00000000-0000-0000-0000-000000000000","title":" ","lyrics":""}"#).await.unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (400, "Give the song a title."));
}

#[tokio::test]
async fn do_action_unknown_action_is_400() {
    assert_eq!(test_app().do_action("nope", b"{}").await.unwrap_err().status, 400);
}
```

- [ ] **Step 2:** `cargo test app::` → FAIL (module missing).
- [ ] **Step 3:** Move the code as specified in Interfaces. `test_app()` is a `#[cfg(test)]` helper using `std::env::temp_dir().join(uuid)`.
- [ ] **Step 4:** `cargo test` → all old tests plus the 3 new ones pass. Run the Tauri app with `dev-build.cmd`, add a song, take a verse live, open the room screen: works as before.
- [ ] **Step 5:** Commit `git commit -m "refactor: move App and actions out of the HTTP layer"`.

### Task 5: Typed update stream

**Files:**
- Modify: `src/app.rs`, `src/server.rs`

**Interfaces:**
- Consumes: `App` from Task 4.
- Produces:
  - `#[derive(Clone, Debug)] pub enum Update { Live(Arc<LiveView>), Data, Job { id: Uuid, percent: u8 } }`
  - `#[derive(Serialize, Clone, Debug)] pub struct LiveView` with exactly the fields `live_json()` emits today (`version, committed, pending, frame, cursor, item, slide, next, media, audio, notice, theme, logo`); `App::live_view(&self) -> LiveView`.
  - `App::tx: broadcast::Sender<Update>`; `App::subscribe(&self) -> broadcast::Receiver<Update>`.
  - SSE handlers serialize `Update` to the same JSON as today (`{"type":"live",...}`, `{"type":"data"}`, `{"type":"job","id","percent"}`), so the web pages and phone remote are unchanged.

- [ ] **Step 1: Write the failing tests:**

```rust
#[tokio::test]
async fn a_live_command_publishes_one_live_update() {
    let app = test_app();
    let mut rx = app.subscribe();
    app.command(Envelope { id: "c1".into(), base: None, cmd: Command::ClearAll });
    assert!(matches!(rx.try_recv().unwrap(), Update::Live(_)));
    assert!(rx.try_recv().is_err());
}

#[test]
fn live_update_json_matches_the_old_wire_format() {
    let app = test_app();
    let v: serde_json::Value = serde_json::from_str(&update_json(&Update::Live(Arc::new(app.live_view())))).unwrap();
    assert_eq!(v["type"], "live");
    for k in ["version","committed","pending","frame","cursor","item","slide","next","media","audio","notice","theme","logo"] { assert!(v.get(k).is_some(), "{k}"); }
}
```

- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement; `pub fn update_json(u: &Update) -> String` in `server.rs`. Remove `live_json`.
- [ ] **Step 4:** `cargo test` → pass. Manual: Tauri app operator, room and phone remote still update live.
- [ ] **Step 5:** Commit `git commit -m "refactor: typed live/data/job update stream"`.

### Task 6: Text layout and fit (`render::layout`)

**Files:**
- Create: `src/render/mod.rs`, `src/render/layout.rs`, `assets/fonts/NotoSans-{Regular,Medium,SemiBold}.ttf`, `assets/fonts/OFL.txt`
- Modify: `src/lib.rs`, `Cargo.toml` (add `cosmic-text`)

**Interfaces:**
- Consumes: `model::Theme`, `live::SlideLayer::Text { text, footer, reference, scripture }`.
- Produces:
  - `pub struct Fonts` (wraps `cosmic_text::FontSystem` loaded with system fonts plus the three bundled files via `include_bytes!`); `Fonts::new() -> Fonts`.
  - `pub struct TextSlide<'a> { pub text: &'a str, pub footer: Option<&'a str>, pub reference: Option<&'a str>, pub scripture: bool }`
  - `pub struct TextLayout { pub font_px: f32, pub font_fallback: bool, pub body: cosmic_text::Buffer, pub foot: Option<cosmic_text::Buffer>, pub body_box: Rect, pub lower: bool }` where `Rect { x, y, w, h: f32 }` in canvas pixels.
  - `pub fn layout(fonts: &mut Fonts, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> TextLayout` — all sizes scale by `canvas.0 / 1920.0`.
  - `pub fn fit_steps(max: f32) -> impl Iterator<Item = f32>` — max, then −4 while > 60, else −2, ending at 26.

- [ ] **Step 1: Write the failing tests** in `layout.rs`:

```rust
#[test] fn fit_steps_match_render_js() {
    let s: Vec<f32> = fit_steps(84.0).collect();
    assert_eq!(&s[..7], &[84., 80., 76., 72., 68., 64., 60.]);
    assert_eq!(s[7], 58.); assert_eq!(*s.last().unwrap(), 26.);
}
#[test] fn short_lyric_keeps_theme_size() { assert_eq!(lay("Amazing grace").font_px, 84.0); }
#[test] fn long_lyric_shrinks_but_not_below_26() {
    let l = lay(&"How sweet the sound that saved a wretch like me ".repeat(12));
    assert!(l.font_px < 84.0 && l.font_px >= 26.0);
}
#[test] fn overlong_lyric_stops_at_26_and_keeps_every_word() {
    let text = "word ".repeat(2000);
    let l = lay(&text);
    assert_eq!(l.font_px, 26.0);
    assert_eq!(laid_out_words(&l.body), 2000);
}
#[test] fn unbreakable_word_wraps_inside_the_box() {
    let l = lay(&"x".repeat(400));
    assert!(max_line_width(&l.body) <= l.body_box.w + 1.0);
}
#[test] fn missing_font_falls_back_to_bundled() {
    let mut t = Theme::default(); t.font = "No Such Font 123".into();
    assert!(layout(&mut Fonts::new(), &slide("Hi"), &t, (1920, 1080)).font_fallback);
}
#[test] fn scripture_uses_scripture_size_and_left_align() {
    let l = layout(&mut Fonts::new(), &TextSlide { text: "For God so loved", footer: None, reference: Some("John 3:16"), scripture: true }, &Theme::default(), (1920, 1080));
    assert_eq!(l.font_px, 64.0);
    assert!(l.foot.is_some());
}
```

(`lay`, `slide`, `laid_out_words`, `max_line_width` are small test helpers in the same file.)

- [ ] **Step 2:** `cargo test render::` → FAIL.
- [ ] **Step 3:** Implement. Fitting: for each size from `fit_steps`, shape the body into `body_box` with `Wrap::WordOrGlyph`; stop at the first size where total height ≤ `body_box.h − 4` and no line exceeds `body_box.w + 1`. `theme.uppercase && !scripture` uppercases the text. Font: use `theme.font` if the font database has that family, else Noto Sans with `font_fallback = true`.
- [ ] **Step 4:** `cargo test` → pass.
- [ ] **Step 5:** Commit `git commit -m "feat(render): text layout and auto-fit matching the web renderer"`.

### Task 7: Rasterizing, shadow and golden images (`render::raster`)

**Files:**
- Create: `src/render/raster.rs`, `src/render/color.rs`, `tests/render_golden.rs`, `tests/golden/*.png`
- Modify: `Cargo.toml` (add `tiny-skia`; dev-dependency `image` with `png`)

**Interfaces:**
- Consumes: `layout`, `Fonts`, `TextSlide` from Task 6.
- Produces:
  - `pub struct Rendered { pub width: u32, pub height: u32, pub rgba_premul: Vec<u8>, pub font_px: f32, pub font_fallback: bool }`
  - `pub fn render_text(fonts: &mut Fonts, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> Rendered` — transparent background; text in `theme.color`; when `theme.shadow`, a blurred black shadow under the body (offset 0,3 blur radius 14 at 0.85 alpha, plus blur 3 at 0.9) and under the footer/reference (offset 0,2 blur 8 at 0.9); lower-third gradient when `theme.position == "lower"`. Shadow sizes scale with canvas width / 1920.
  - `pub fn parse_color(s: &str) -> [u8; 4]` — `#rgb`, `#rrggbb`, `#rrggbbaa`, CSS named colors via a small table (`white, black, red, yellow, …` the 16 basic names); anything else → white.

- [ ] **Step 1: Write the failing tests** in `tests/render_golden.rs`, one per case: `golden_short_lyric`, `golden_long_lyric_fitted`, `golden_scripture_with_reference`, `golden_lower_third`, `golden_uppercase_no_shadow`, each rendering at 1920×1080 with bundled fonts only (theme font "Noto Sans") and comparing to `tests/golden/<name>.png`: pass when mean absolute channel difference ≤ 2 and ≤ 0.5% of pixels differ by more than 32. With `UPDATE_GOLDENS=1` the test writes the PNG instead. Also:

```rust
#[test] fn empty_slide_is_fully_transparent() { assert!(render(" \n ").rgba_premul.iter().all(|&b| b == 0)); }
#[test] fn non_latin_text_draws_pixels() { assert!(render("Sublime gracia del Señor 놀라운 은혜 🙏").rgba_premul.chunks(4).any(|p| p[3] > 0)); }
#[test] fn bad_colors_fall_back_to_white() {
    assert_eq!(parse_color("#zz"), [255,255,255,255]); assert_eq!(parse_color(""), [255,255,255,255]);
    assert_eq!(parse_color("red"), [255,0,0,255]); assert_eq!(parse_color("#0f0"), [0,255,0,255]);
}
#[test] fn thumbnail_scales_font_with_canvas() { assert_eq!(render_at("Amazing grace", (480, 270)).font_px, 21.0); }
```

- [ ] **Step 2:** `cargo test --test render_golden` → FAIL.
- [ ] **Step 3:** Implement. Glyphs come from `cosmic_text::SwashCache`, blended into a `tiny_skia::Pixmap`. Shadow: draw glyph alpha into a separate mask, two-pass box blur (three passes approximating Gaussian), tint black, composite under the text.
- [ ] **Step 4:** Generate goldens with `UPDATE_GOLDENS=1 cargo test --test render_golden`, open each PNG and compare by eye against the Tauri app's room output for the same slide (screenshot side by side, stored in `tests/golden/reference/` for the reviewer). Then `cargo test` → all pass.
- [ ] **Step 5:** Commit `git commit -m "feat(render): rasterize slides with shadows; golden-image tests"`.

### Task 8: Render cache and off-thread rendering (`render::cache`)

**Files:**
- Create: `src/render/cache.rs`

**Interfaces:**
- Consumes: `render_text`, `Rendered` from Task 7.
- Produces:
  - `pub struct RenderKey { text: String, footer: Option<String>, reference: Option<String>, scripture: bool, theme: Theme, canvas: (u32, u32) }` (`Hash + Eq`; `Theme` gains `Hash, Eq` derives).
  - `pub struct Renderer` — owns `Fonts` on one dedicated worker thread; LRU of `Arc<Rendered>` capped at 256 MB of pixels.
  - `Renderer::new() -> Arc<Renderer>`; `pub async fn render(&self, key: RenderKey) -> Arc<Rendered>`; `pub fn cached(&self, key: &RenderKey) -> Option<Arc<Rendered>>`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[tokio::test] async fn second_request_is_served_from_cache() {
    let r = Renderer::new(); let k = key("Amazing grace", (1920, 1080));
    let a = r.render(k.clone()).await; let b = r.render(k).await;
    assert!(Arc::ptr_eq(&a, &b));
}
#[tokio::test] async fn cache_evicts_oldest_past_the_cap() {
    let r = Renderer::with_cap(2 * 1920 * 1080 * 4);
    for t in ["a", "b", "c"] { r.render(key(t, (1920, 1080))).await; }
    assert!(r.cached(&key("a", (1920, 1080))).is_none());
    assert!(r.cached(&key("c", (1920, 1080))).is_some());
}
#[tokio::test] async fn many_concurrent_requests_all_complete() {
    let r = Renderer::new();
    let all = futures_util::future::join_all((0..64).map(|i| r.render(key(&format!("line {i}"), (480, 270))))).await;
    assert_eq!(all.len(), 64);
}
```

- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement: requests go over a `std::sync::mpsc` channel to the worker thread, replies via `tokio::sync::oneshot`; `with_cap(bytes)` constructor for tests.
- [ ] **Step 4:** `cargo test` → all pass.
- [ ] **Step 5:** Commit `git commit -m "feat(render): cached off-thread slide renderer"`.

---

**After this plan:** update `PROJECT_PLAN.md` (architecture: native Slint selected October 29; work board rows for Plans 1–4; check results C1–C5), then write Plan 2 from the recorded check results.
