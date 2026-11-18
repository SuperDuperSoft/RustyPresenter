# GPU rendering and two-screen lag: design decision

Date: 2026-11-03. Branch: `perf-gpu`. Status: decided, Phase 1 not started.

## For Jake (plain language)

**What is wrong.** RustyPresenter draws all three windows (your control
screen, the projector, the stage screen) from one worker. That worker can
paint only one window at a time, and after each paint it waits for that
monitor to finish its screen refresh. Windows always hands it the window
on top first, which is your control screen. So whenever the control screen
keeps asking to be redrawn (a fade, moving the mouse over buttons, the
video progress bar), the projector has to wait in line. In our tests the
projector picture arrived about 0.4 seconds late and the crossfade was
never seen. That is why it only happens with two screens: with one screen
there is no second window waiting in line.

**Should we switch to Direct3D / Metal / Vulkan?** Not now. The problem is
the waiting in line, not the drawing itself. A different graphics system
would still wait in the same line. We actually tried the "native GPU" route
(Direct3D 12 through wgpu) and it was much worse: 1 to 2 frames per second.
Also, the video player we use (mpv) can currently only hand video to us
through OpenGL, so switching would break video under lyrics.

**What we will do first.** Small, safe changes, each one measured:
1. Stop things from animating in a way that makes *every* window redraw.
   The projector does its own fade; the control screen stops redrawing
   just because the projector fades.
2. Only the projector waits for the screen refresh. The control and stage
   screens stop waiting.
3. The projector window goes "on top" (it is on its own monitor, so you
   will not notice) so it gets painted first.
4. Make each projector frame cheaper to draw.

**What you will check.** After Phase 1, with both screens on: press Next
quickly a few times and wiggle the mouse over the control screen. The
projector should change right away with a smooth fade, and the control
screen should stay responsive. Also look closely at the projector for any
tearing (a horizontal line through moving video).

**Later.** Direct3D / Metal comes back on the table only if we want
video-to-video crossfades, or if old Intel laptops turn out to have broken
OpenGL. Even then it only goes ahead if it measures better on your desktop.

## Root causes, with evidence

Measured on a Ryzen 5600H laptop (Radeon iGPU and RTX 3050 Ti, one
1920x1080 120 Hz screen) with a probe that opens the real operator window,
a 2560x1080 room and a stage window and presses Next once a second, plus
`examples/slide_bench.rs` (commits 14ff3c9, 6ffb6b9). Jake's two-monitor
RTX 2060 desktop has not been measured yet; that is the first Phase 1 step.

### 1. Paint starvation on the shared UI thread (main cause)

- All windows are painted on one UI thread. winit 0.30.13 on Windows stops
  reading messages after every `RedrawRequested`
  (`platform_impl/windows/event_loop/runner.rs:208-214`,
  `event_loop.rs:389-434`), so at most one window paints per loop turn.
- Windows hands out `WM_PAINT` highest-z-order first: the focused operator.
- Slint 1.18.1 FemtoVG forces `SwapInterval::Wait(1)` on every window
  (`i-slint-backend-winit-1.18.1/renderer/femtovg/glcontext.rs:208-214`),
  so every painted window blocks the thread for up to one vblank. On NVIDIA
  an operator frame took ~5 ms to draw then ~11 ms in the swap.
- Measured: room's first frame after a slide swap came +356 to +451 ms late
  (after the operator's 400 ms monitor fade ended) in nearly every change
  under CPU load, on both GPUs; intermittently on a quiet machine. The room
  got 3-5 frames per change; the stage 2-5 per 400 ms fade; the operator
  30-50.
- Confirmation: making the room AlwaysOnTop reversed it (room 36-49
  frames, operator 4-34).
- Two monitors make it worse: each window waits for its own monitor's
  vblank, and Jake's 75 Hz monitor is out of step with the other one.
  Forum reports (Khronos, JUCE, NVIDIA) describe exactly this with vsynced
  GL windows on mixed-refresh monitors. Confidence it is Jake's case:
  medium, until measured on his desktop.

### 2. Slint's "something is animating" flag is global

- `i-slint-core-1.18.1/api.rs:792-795` (`has_active_animations`, comment
  "TODO make it really per window"); the winit backend
  (`event_loop.rs:246-257`) then requests a redraw of **every** window.
- Animations in the app: `ui/room.slint:38,45` (400 ms crossfade), `:109`
  (black, 200 ms), `ui/operator/monitor.slint:73` and the monitor's reused
  `FadeLayer`, `ui/widgets.slint:330` (transport `Bar`, `animate width
  500ms`, restarted by every position report), topbar focus, toasts.
- Effect: every slide change makes all windows redraw fully at refresh
  rate for 400+ ms. During any video or audio playback the Bar keeps an
  animation running, so all windows redraw continuously (from reading the
  code; not yet measured).
- Benchmark: FemtoVG with 3 windows used 166-209% of one core while
  changing slides every 250 ms (operator alone: 63%). Idle: 0 fps, 0.9%.

### 3. Expensive room frame right after a swap (on the UI thread)

- Each `Face` (`room.slint:12-26`) is a Rectangle with a child Image under
  an animated opacity. `Opacity::need_layer` (`i-slint-core items.rs:955-971`)
  is true for any opacity other than 1 when there is a child, so FemtoVG
  renders each such face into a full-size offscreen texture (up to 8).
- `room.rs:303-313` (`layer`, with `put` at 54-58) replaces the whole
  `Layer` struct, probably dirtying both faces' images; pixel-buffer images
  have no texture cache key (`graphics/image.rs:345-363`), so two 10.6 MB
  textures are re-uploaded at 2560x1080.
- Measured: first room frame 8-11 ms (iGPU), 12-37 ms (NVIDIA), outliers
  50-108 ms. The operator is frozen during it.

### 4. Text rendering adds latency, not lag

- Rendering is off the UI thread. 2560x1080 slide: 32-87 ms (first one
  400-570 ms, font load), then a 10.6 MB `clone_from_slice` (2-15 ms,
  `room/prepare.rs:95`). A new OS thread per plan (`room.rs:162`) queues on
  one FIFO renderer with no cancel, so fast Next presses queue up.
- Model/command code is cheap: `apply_live` 0.02-0.4 ms, stage fit
  0.24-0.33 ms, no UI-thread locks. Optimising Rust model code will not
  fix the lag.

### 5. What switching graphics API would and would not do

- Starvation, the global flag and one-paint-per-turn live in Slint/winit,
  not in OpenGL. A new API keeps all three.
- `femtovg-wgpu` (D3D12/Vulkan/Metal) in slide_bench: 3 windows, p50
  latency up to 5.3 s, longest UI gap 14 s, room ~1 fps. 1 window: 110-330
  stalls. (Optimus laptop may contribute; recheck on Jake's desktop.)
- Skia (D3D/Metal/Vulkan via wgpu) cannot be built on our MinGW toolchain
  (no prebuilt rust-skia binaries for windows-gnu; source build failed),
  needs MSVC and adds a heavy binary. Not measured.
- libmpv 0.41's render API is OpenGL or software only. A D3D11 render API
  landed in mpv master (4255130de4, 2026-10-15) for the unreleased 0.42;
  there is no Metal or Vulkan one.
- Slint 1.18 only accepts foreign GPU textures through `unstable-wgpu-30`
  (`Image::try_from(wgpu::Texture)`); the feature name changes every minor.

Benchmark table (laptop, median of 3, first clean matrix):

| renderer | win | latency p50/p95/max ms | stalls >20 ms | longest gap ms | CPU % one core | fps op/room/stage |
|---|---|---|---|---|---|---|
| femtovg | 1 | n/a | 0 | 11 | 63 | 106/-/- |
| femtovg | 3 | 14/20/25 | 0 | 12 | 195 | 105/103/61 |
| femtovg-wgpu | 1 | n/a | 166 | 5170 | 67 | 32/-/- |
| femtovg-wgpu | 3 | 5316/9583/10083 | 28 | 14182 | 63 | 2/1/0.3 |
| software | 1 | n/a | 0 | 10 | 41 | not countable |
| software | 3 | 19/31/40 | 0 | 19 | 91 | not countable |

These runs were on one monitor and some were disturbed by parallel builds;
the femtovg 3-window row does not show the starvation that the probe saw
under load. That is why Phase 1 starts with a clean baseline on Jake's
desktop.

## Decision: architecture per platform

| Platform | Slint renderer | Video under text | Fallback |
|---|---|---|---|
| Windows 10+ | winit + FemtoVG, OpenGL | mpv OpenGL render API, drawn under the scene in the rendering notifier (`src/video/surface.rs`, check C1) | `RP_FORCE_SOFTWARE`: Slint software renderer + mpv SW |
| macOS 12+ (Apple silicon) | winit + FemtoVG, OpenGL | same underlay | same |
| Linux | winit + FemtoVG, OpenGL | same underlay | same |

Rules that come with it:

1. **Only the room waits for vsync.** Operator and stage swap without
   waiting; Slint's frame throttle caps their rate.
2. **No Slint `animate` on anything that changes often** while the
   animation flag is global. Fades are driven from Rust per window.
3. **The room wins paint order** when it is on its own monitor.
4. **Upload each picture once; touch only the face that changes; no
   offscreen layers** (opacity on childless items).
5. Native GPU APIs are a gated later phase (Phase 3), not the default.

Rejected for now: femtovg-wgpu (measured collapse), Skia (toolchain,
binary size, unmeasured), mpv in a separate window under a transparent
overlay (z-order, focus, Wayland, no video crossfade), writing D3D/Metal/
Vulkan directly (Slint already abstracts it).

## Phased plan

Every step is measured with `examples/slide_bench.rs` before and after, on
a quiet machine (no cargo builds running). Steps land as separate commits
so each effect is attributable.

### Phase 0: free checks and baseline on Jake's desktop (no code)

1. Build: `cargo build --release --features renderer-wgpu --example slide_bench`.
2. With no builds running, the room full screen on the 2560x1080 monitor:
   `RP_BENCH_ROOM_MONITOR=2 slide_bench.exe --matrix 3`, and again with
   `RP_BENCH_INTERVAL_MS=100`. Save the output.
3. Repeat the femtovg 3-window run with: the 75 Hz monitor set to 60 Hz;
   NVIDIA Control Panel "Vulkan/OpenGL present method = Prefer layered on
   DXGI swapchain"; NVIDIA "Vertical sync = Off" for the exe. Restore after.
4. Run the real app once with `SLINT_DEBUG_PERFORMANCE=refresh_lazy,console`.

If 3 makes the lag vanish, the vsync mechanism is confirmed on his machine.

### Phase 1: remove the two-screen lag (days, low risk, no new deps)

Order puts the cheapest fixes for the starvation mechanism first.

**1.1 Bench additions** (before any fix, so we have a baseline)
- Metric: room first-frame delay after each swap (ms).
- Metric: frames per window per slide change.
- Scenario `RP_BENCH_SCENARIO=playback`: one clip playing in the room, no
  slide changes; record fps per window and CPU.
- Scenario `RP_BENCH_SCENARIO=hover`: synthetic mouse moves over the
  operator while slides change; record room frames and first-frame delay.

**1.2 Swap interval 0 for operator and stage** (new `src/ui/pacing.rs`)
- Rendering notifier on operator and stage; in `RenderingSetup`, load and
  call `wglSwapIntervalEXT(0)` (Windows), `glXSwapIntervalEXT` /
  `eglSwapInterval` (Linux), `CGLSetParameter(kCGLCPSwapInterval, 0)`
  (macOS) via `get_proc_address`/`dlsym`. Missing extension = no-op + log.
- The room keeps interval 1; VideoSurface already owns its notifier, so it
  gets an explicit `vsync: bool` and sets 1.
- Check the frame throttle really caps hover-storm repaints; if CPU goes
  up, add a frame cap.

**1.3 Room paints first**
- In `src/ui/room.rs`, set the room window AlwaysOnTop only when it is
  full screen on a different monitor from the operator. Settings toggle to
  turn it off. Never when the room shares the operator's monitor.

**1.4 Stop global redraw storms**
- `ui/widgets.slint:330`: remove `animate width` from `Bar` (step width
  directly); throttle position reports to about 4 per second.
- `ui/room.slint:38,45,109` and `ui/operator/monitor.slint:73`: remove
  `animate opacity`; faces take plain opacity properties.
- New `Fader` (`src/ui/fade.rs`): one `slint::Timer` per window, ~16 ms
  tick, opacity computed from elapsed `Instant` time (400 ms on any refresh
  rate), clamped, eased by hand, starts from the current opacity when a new
  slide arrives mid-fade, calls `request_redraw()` on its own window only,
  stops the timer when done.
- Operator monitor: instant cut by default (1-2 operator frames per
  change); a short fade is a later product option using the same Fader.

**1.5 Cheaper room frames**
- Restructure `Face`: the Image carries its own opacity and has no
  children; the colour fill is a separate childless Rectangle. No
  offscreen layer.
- Replace the whole-`Layer` struct writes (`room.rs:303-313`, `put`) with
  per-face properties (`a-image`, `b-image`, `a-op`, `b-op`, ...); write
  only the face that changes. Faces at opacity 0 get `visible: false`.
- Confirm by counting GL texture uploads in the probe.

**1.6 Rapid Next presses**
- Generation token in `src/render/cache.rs`: drop stale renders before
  they start.
- One long-lived room-prepare worker instead of a thread per plan
  (`room.rs:162`).
- Build the `SharedPixelBuffer` directly in the rasterizer instead of
  `clone_from_slice` (`room/prepare.rs:95`).
- Prefetch next and previous slide renders while idle.

**1.7 Slint double-draw fix**
- Take slint#14282 (timer throttle draws every change twice on Windows):
  newer Slint if released, else `[patch.crates-io]` of
  `i-slint-backend-winit`. Check whether the new Slint makes
  `has_active_animations` per window. Drop the patch once released.

**Phase 1 pass criteria** (Jake's desktop, `RP_BENCH_ROOM_MONITOR=2
--matrix 3`, femtovg, quiet machine):

| Measure | Today (laptop) | Target |
|---|---|---|
| Room first frame after swap | +356..+451 ms under load | within one vblank (~13 ms at 75 Hz), at most 30 ms |
| Room frames per 400 ms fade | 3-5 | about 30 at 75 Hz |
| Operator frames per room-only change | 30-50 | 1-2 |
| Stalls > 20 ms | 0 quiet, many under load | 0 |
| CPU during changes, 3 windows | ~195% of one core | under ~60% |
| Idle | 0 fps, ~1% CPU | unchanged |
| Playback scenario | all windows redraw (by code reading) | only the room redraws, at the video frame rate |
| Hover scenario | room starved | room frame count unchanged vs no hover |

Manual check on Jake's two-screen desktop: operator on one monitor, room
full screen on the 2560x1080 one. Press Next 10 times slowly then 10 times
fast, while moving the mouse over the operator; the projector must change
at once with a visible smooth crossfade. Play a video with lyrics over it;
look for tearing on the projector and stutter on the control screen.
Repeat the bench with `RP_FORCE_SOFTWARE=1`.

### Phase 2: low-end hardening (after Phase 1 is measured)

- Byte-capped slide LRU (about 64 MB on machines with 4 GB RAM or less).
- Log mpv `hwdec-current`; warn when an Intel iGPU falls back to CPU decode.
- Make sure an mpv wake redraws only the window that shows that player.
- Software video path: hand RGBA straight to Slint without `show_sw`'s
  per-pixel loop and per-frame buffer allocation.
- Only if uploads still show in the numbers: rasterize only the text's
  bounding box and position it, instead of a full-canvas 10.6 MB image.
- Only if the room still starves after Phase 1: room in its own process
  (`rustypresenter --room` over a local pipe; ~60-100 MB more RAM).
- Verify: same matrix on an old Intel iGPU laptop with 4 GB RAM, and with
  `RP_FORCE_SOFTWARE=1`; targets as in Phase 1.

### Phase 3: native GPU APIs, gated (only where the evidence pays)

Start only if one of these becomes a real need: video-to-video crossfades,
or old Intel OpenGL drivers that are broken or slow. And only if the gate
passes.

- **Gate:** move the Windows build to MSVC, build Skia with
  `unstable-wgpu-30`, run `slide_bench --matrix 3` with
  `RP_BENCH_ROOM_MONITOR=2` for skia vs femtovg on Jake's RTX 2060 and an
  old Intel iGPU. Continue only if Skia's latency, stalls and CPU are at
  least as good as FemtoVG's with 3 windows, and the binary-size increase
  is acceptable.
- **Windows:** Skia on wgpu (D3D12). Video as a texture shown in the room's
  existing `video-frame` Image: today via mpv 0.41 OpenGL on a hidden WGL
  context + `WGL_NV_DX_interop2` (reference: mpv-engine 0.2.0); later mpv
  0.42 `MPV_RENDER_API_TYPE_D3D11` into a keyed-mutex shared texture. mpv
  renders on its own thread. Feature flag; FemtoVG GL underlay remains the
  automatic fallback (pre-Haswell Intel has no D3D12).
- **macOS:** stay on the GL underlay unless Apple removes OpenGL; then
  mpv GL into IOSurface, imported into Metal via wgpu-hal.
- **Linux:** stay on OpenGL (Mesa is first-class).
- Verify: the same matrix plus a 1080p30 clip in the room while slides
  change (CPU, mpv `frame-drop-count`, room fps); pass = no stalls, room at
  refresh rate during fades, video CPU at or below the GL underlay.

## Risks

- **Swap interval 0 on the operator** could let it spin faster than needed
  under hover storms; mitigated by the throttle check and a frame cap. It
  may tear on Linux X11 without a compositor (acceptable for the control
  screen). Windows DWM and macOS composite, so no tearing expected.
- **The room's own vsync swap still blocks the UI thread** up to ~13 ms per
  room frame during fades. If operator input latency stays visible, the
  room-in-own-process step (Phase 2) is the real decoupling.
- **AlwaysOnTop** could cover dialogs if misdetected; only on a separate
  monitor, with a toggle.
- **Hand-written fades** must handle mid-fade interruption and refresh
  rates; they replace six lines of declarative `animate` with ~150 lines
  of Rust (testable).
- **Swap-interval FFI** is small but unsafe, per platform; macOS path is
  untested.
- **Texture-saving claim** (single-face updates) comes from reading Slint
  code; confirm with an upload counter.
- **`[patch.crates-io]` fork** must be rebased on Slint upgrades; drop it
  once a release has #14282.
- **Evidence is from a one-monitor laptop**, partly under build load.
  Phase 0 on Jake's desktop can change priorities.
- **Phase 3** risks: MSVC move, Skia binary size, `unstable-wgpu-NN` churn,
  interop driver bugs on Intel/AMD/NVIDIA, three video paths to maintain.

## Cost

- Phase 1: no new dependencies, no binary size change, MinGW stays.
  Roughly 400-600 changed lines (fade.rs, pacing.rs, room.slint/room.rs
  refactor, render cancel, bench scenarios). Days of work.
- Phase 2: days, mostly small independent changes.
- Phase 3: 2-3 weeks for Windows, 1-2 for macOS, plus ongoing testing of
  several render/video combinations each release.

## Sources

- Trace probe sources, logs and summarizer:
  `%TEMP%\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\04fc570e-a63a-4401-ac72-a132067dae90\scratchpad\probe`
- Benchmark: `examples/slide_bench.rs` (commits 14ff3c9, 6ffb6b9).
- Slint 1.18.1 `glcontext.rs`, `frame_throttle.rs`, `api.rs`; slint#14281
  and PR #14282; slint#8693, #12030; Slint renderer docs.
- mpv `render.h`, commit 4255130de4 (D3D11 render API); mpv-engine 0.2.0.
- Khronos forum threads on multi-window SwapBuffers and mixed-refresh
  vsync; NVIDIA "Prefer layered on DXGI swapchain" reports.
