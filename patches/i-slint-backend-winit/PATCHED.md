# i-slint-backend-winit 1.18.1, patched

The published crate, unchanged except `frame_throttle.rs`: Slint pull request 14282
("winit: Draw each change once with the timer frame throttle", merged 2026-10-20, merge commit
881a0bc4508b748d13c3892bba0019c0e3d334a6). Without it, the timer frame throttle (Windows, X11) waits one refresh
before the first redraw and then draws every change twice.

Used through `[patch.crates-io]` in the top-level `Cargo.toml`. Delete this folder and that entry once a Slint
release includes the fix (1.19 or later), and check that the release still sets `SwapInterval::Wait(1)` in
`renderer/femtovg/glcontext.rs` (RustyPresenter turns it off per window in `src/ui/pacing.rs`).
