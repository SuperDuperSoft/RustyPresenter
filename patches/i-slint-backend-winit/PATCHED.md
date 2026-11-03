# i-slint-backend-winit 1.18.1, patched

The published crate, unchanged except `frame_throttle.rs`: Slint pull request 14282
("winit: Draw each change once with the timer frame throttle", merged 2026-10-20, merge commit
881a0bc4508b748d13c3892bba0019c0e3d334a6). Without it, the timer frame throttle (Windows, X11) waits one refresh
before the first redraw and then draws every change twice.

The rest is a byte-for-byte copy of the crates.io 1.18.1 package (only its `Cargo.lock`, `.cargo-ok` and
`.cargo_vcs_info.json` left out), so it builds offline from this folder; `Cargo.lock` pins it as a path package at
exactly 1.18.1, the version every `i-slint-*` 1.18.1 crate requires with `=1.18.1`. The licence is the crate's own
(`GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`, texts in `LICENSES/`), the
same terms as the rest of Slint that the app already uses; the copyright headers are kept.

To check the copy: `git diff --no-index <cargo registry>/i-slint-backend-winit-1.18.1 patches/i-slint-backend-winit`
shows only `frame_throttle.rs` (and this file).

Used through `[patch.crates-io]` in the top-level `Cargo.toml`. Delete this folder and that entry once a Slint
release includes the fix (1.19 or later), and check that the release still sets `SwapInterval::Wait(1)` in
`renderer/femtovg/glcontext.rs` (RustyPresenter turns it off per window in `src/ui/pacing.rs`).
