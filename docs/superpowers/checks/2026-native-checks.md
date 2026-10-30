# Native app feasibility checks (C1–C5)

Spec: `docs/superpowers/specs/2026-10-29-native-slint-design.md` §12. Throwaway code lives in `experiments/`.
Slint version used: 1.18.1.

| Check | OS | Result | Notes |
|---|---|---|---|
| C1 mpv under Slint (OpenGL) | all | Not run | Needs libmpv (LGPL build) on this PC; waiting for the user's OK to download it. |
| C1 fallback (software) | all | Not run | Same. |
| C2 monitors + borderless fullscreen | Windows 11 (one 1920×1080 monitor, 125% scale) | **PASS** | `slint` feature `unstable-winit-030`; `with_winit_window` → `available_monitors()`, `Fullscreen::Borderless(Some(m))`. Window covered the monitor exactly. Monitor names are `\\.\DISPLAY1` etc., the same ids saved in `settings.displays` today, so existing choices carry over on Windows. Second-monitor case still to check on the church laptop. |
| C2 | macOS, Linux | Not run | Needs those machines. |
| C3 file drop | all | Not run | Needs a person to drag a file onto the window. |
| C4 Slint testing backend, headless | Windows 11 | **PASS** | `i-slint-backend-testing` 1.18, `init_no_event_loop()`, `ElementHandle::find_by_element_id(&c, "Component::id")`, `invoke_accessible_default_action()`. **Element ids need `SLINT_EMIT_DEBUG_INFO=1` at compile time** (and a rebuild after setting it). Lookup by accessible label did not find a `Button` by its text. |
| C4 | macOS, Linux | Pending CI | Added to CI; runs on the first push. |
| C5 LibreOffice → PDF → pdfium | all | Not run | Needs LibreOffice and pdfium on this PC; waiting for the user's OK. |
