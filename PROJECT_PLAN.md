# RustyPresenter — shared project plan

**Status:** Building. The user approved the design and asked for the app to be built on October 29, 2026. v0.2 (Tauri) is in `src/` and `web/`; the native Slint app is being built beside it (Plans 1 and 2 done on Windows, Plan 3 next). The earlier experiment is in `experiment-archive/`.
**Updated:** November 1, 2026 (native Plan 2 finished: video, room output and stage windows)
**Collaborators:** User, Codex, and Claude
**Purpose:** A Rust-based presentation application for this church's personal use during live services.

## Start here — for Codex and Claude

Read this file before working. It is the shared record of requirements, decisions, work ownership, and handoffs. The existing code is an unfinished experiment, not an approved architecture or a usable release.

- Preserve the user's requirements. Do not quietly replace a required integration with a lesser workflow.
- Keep planning separate from implementation. The user explicitly clarified that we are currently planning.
- Before starting an authorized task, record its owner and the files or area involved in the work board below. Read the other collaborator's latest handoff first.
- Avoid simultaneous edits to the same files. Agree on shared interfaces before implementing connected components.
- Record discoveries, decisions, checks performed, and unresolved issues here. Distinguish working functionality from designs, placeholders, and untested code.
- Review each other's work. A collaborator's completion claim alone is not release verification.
- Do not put account passwords, access tokens, API keys, or downloaded private church content in this plan or source control.
- Only the user can change product priorities. Both collaborators may propose changes and explain tradeoffs.

No tasks have been dispatched to Claude through this plan. The ownership split below is a proposal for coordination.

## 0. What controls v1

**`docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md` controls v1 behavior.** This plan holds the long-term requirements, collaboration rules, feasibility evidence, and the work board. If this plan and the spec disagree about v1, the spec wins; fix the plan.

User decisions (October 29, 2026 design review):

- Church currently uses **EasyWorship**, not ProPresenter. Goal is to save money.
- ~~v1 is Windows only~~ → **Superseded October 29–30:** the user asked for a **native app on Windows, macOS and Linux**, no browser.
- ~~Architecture: Tauri 2~~ → **Superseded:** native **Slint** UI (user holds the Slint Royalty-Free License v2.0), video through **libmpv**, PowerPoint import through PowerPoint on Windows or LibreOffice everywhere. Design: `docs/superpowers/specs/2026-10-29-native-slint-design.md`, which overrides the v1 spec's platform sections. The Tauri app keeps working until the native app reaches parity.
- **Phone remote: browser page** for v1.
- Displays: operator laptop screen, room output, optional **foldback** monitor.
- Backgrounds: color, image, looping video. **Video items with audio**, still-image items, and **audio items**.
- **Clear All / Slide / Media / Audio + Logo**, defined by the spec's clear matrix. No Blackout/Restore.
- **Next on the last slide takes the next service item live.**
- **Songs in v1:** SongSelect `.txt` import + one-time EasyWorship library import (songs only). Direct SongSelect login is deferred, not dropped.
- **Bible:** all six translations required for v1 release; the user may approve an exception after seeing cost/terms.
- **Decks:** PowerPoint and Google Slides as slide images; PowerPoint renders via the installed PowerPoint.

## 1. Requirements

| ID | Requirement | v1 (per spec) | Long-term |
| --- | --- | --- | --- |
| R1 | Rust-based desktop app | **Windows** build, cross-platform-friendly code | Windows, macOS, Linux |
| R2 | Live presentation with safe preview | Preview/committed separation, clear matrix, generations | Same |
| R3 | Android and iOS remote control | Browser remote with secured pairing | Optional native apps |
| R4 | SongSelect song import | `.txt` import + EasyWorship library import (**user-approved interim**) | Direct account login and automatic retrieval, pending CCLI partner access |
| R5 | Easy Bible verse presentation | `Ctrl+B` flow, auto split from settings, easy tweaks | Same |
| R6 | NIV, ESV, CSB, HCSB, NLT, KJV (CSB ≠ HCSB) | All six Ready for release, unless the user approves an exception | Same |
| R7 | PowerPoint import | Slide images via installed PowerPoint | Animations, embedded media, editable text, LibreOffice |
| R8 | Google Slides import | Slide images via system-browser sign-in + Picker | Same plus fidelity upgrades |
| R9 | Personal church use | No public distribution, subscriptions, or cloud hosting | Same |
| R10 | Foldback / stage display | Optional second output: current/next text, notes, clock, stage messages | Layouts, timers |
| R11 | Media | Background color/image/video; video with audio; images; audio items | Livestream output/NDI |

Long-term goals stay goals. Deferring them in v1 does not remove them.

## 2. Live-service experience

Detailed v1 behavior is in the spec: layers and clears (§6), navigation and keyboard focus (§7), Bible (§8), songs and imports (§9), displays (§10), remote (§12). Summary:

1. Build the service plan from songs, scripture, decks, media, and announcements.
2. Selecting an item loads **Preview** only.
3. **Take Live** sends it to the room. Within the live item, slide clicks and arrows change the live slide; Next at the end takes the next service item.
4. **Clear All / Slide / Media / Audio / Logo** per the clear matrix.
5. Edits and imports never change the committed room output. A failure while *preparing* new content keeps the committed output. Media that fails *while playing* is cleared from its layer. A crashed room window recovers per spec §4.2 (static layers return, audible media does not).

## 3. Feasibility checks

| ID | Check | Owner | Status |
| --- | --- | --- | --- |
| F1 | Room output in its own WebView2 environment; process-failure detection and recreation | Unassigned | Planning |
| F2 | Audio device routing, autoplay without a click, device loss, on the church laptop | Unassigned | Planning |
| F3 | Google system-browser OAuth + PKCE, Picker on a loopback page, token lifetime with `drive.file` | Claude — proposed (was FEAS-04) | Planning |
| F4 | EasyWorship version, consistent read-only copy, schema, RTF | Unassigned | Planning |
| F5 | Bible sources for all six (= FEAS-02) | Codex — proposed | Planning |
| F6 | Stable monitor identity on Windows | Unassigned | Planning |
| FEAS-01 | CCLI partner access for direct SongSelect login (long-term; does not gate v1) | Codex — proposed | Planning |

### Bible translation readiness (fill in during F5)

| Translation | Provider | Exact edition | Status | Attribution | Local storage allowed | Cost | Test results |
| --- | --- | --- | --- | --- | --- | --- | --- |
| KJV | eBible.org source (downloaded) | TBD (verify) | Needs setup (parser has failing tests) | Public domain | Yes | Free | 3 failing tests (book mappings) |
| NIV | — | — | Needs setup | — | — | — | — |
| ESV | Crossway API (candidate) | — | Needs setup | — | — | — | — |
| CSB | — | — | Needs setup | — | — | — | — |
| HCSB | — | — | Needs setup | — | — | — | — |
| NLT | Tyndale API (candidate) | — | Needs setup | — | — | — | — |

Statuses: Ready – Full offline, Ready – Prepared offline, Needs setup, Unavailable (spec §8.5). An Unavailable translation does not satisfy R6 unless the user records an exception here.

### Native app checks (C1–C5)

Full results and gotchas: `docs/superpowers/checks/2026-native-checks.md`. F1 and F6 above were Tauri-era checks; C1 and C2 replace them for the native app.

| ID | Check | Windows | macOS / Linux |
| --- | --- | --- | --- |
| C1 | libmpv video under Slint text (OpenGL), 1080p30 | **Pass** (≈30 fps, text stays readable, CPU 30–52% of one core) | Not run |
| C1 fallback | mpv software render into a Slint image | **Pass** (30 fps, CPU 65–77% of one core) | Not run |
| C2 | Monitor list + borderless fullscreen | **Pass** on one monitor; second monitor still to check on the church laptop | Not run |
| C3 | Files dropped onto a Slint window | Not run (needs a person to drag a file) | Not run |
| C4 | Slint testing backend, headless | **Pass** | In CI |
| C5 | LibreOffice → PDF → pdfium slide images | **Pass** (warm conversion ≈ 2 s with a persistent private profile) | Not run |

## 4. Architecture

**Selected (October 29–30):** native Slint app, one process plus a small supervisor for crash recovery. Rust core unchanged (`model`, `live`, `bible`, `songselect`, `slides`, `store`); `App` and its dispatcher live in `src/app.rs` with no axum or Tauri types; one typed `Update` stream feeds the native UI and the phone remote's SSE; slides are drawn by the Rust compositor in `src/render/` (cosmic-text + tiny-skia, golden-image tests); video and audio through libmpv loaded at runtime; the phone remote stays a web page served by `server.rs`. Full design: `docs/superpowers/specs/2026-10-29-native-slint-design.md`.

Plans: Plan 1 foundation (`docs/superpowers/plans/2026-10-29-native-foundation.md`, done); Plan 2 video, room output, stage (`docs/superpowers/plans/2026-11-01-native-video-room-stage.md`, done on Windows); Plan 3 operator window and dialogs (`docs/superpowers/plans/2026-11-01-native-operator.md`); Plan 4 imports, crash recovery, packaging, Tauri removal (`docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md`).

*Previous:* Tauri 2 with WebView2 windows (v0.2, still the working app until the native one replaces it).

## 5. Delivery sequence (v1)

Follows spec §15:

0. Setup (git, move off OneDrive, Tauri shell, audit experiment) + F1, F2, F6; F3–F5 in parallel.
1. Live core.
2. Songs (TXT, EasyWorship after F4).
3. Bible (KJV first; release gate after F5).
4. Phone remote + pairing security.
5. Decks (PowerPoint, then Google after F3).
6. Acceptance matrix + rehearsal.

No implementation starts until the user approves the written spec and the implementation plan.

## 6. Acceptance

v1 acceptance is the runtime matrix in spec §14.2 plus the core unit tests in §14.1. Long-term additions: macOS/Linux builds with multi-display checks; direct SongSelect login retrieving a song with metadata.

This is not yet ready to replace the church's existing presentation setup.

## 7. Codex / Claude work split

| Area | Proposed lead | Reviewer |
| --- | --- | --- |
| Requirements, spec, acceptance criteria | Shared with user | Both |
| Bible sources (F5), CCLI access (FEAS-01) | Codex | Claude |
| Rust core, persistence, remote security | Codex | Claude |
| Operator UI and live workflows | Claude | Codex |
| Deck imports (PowerPoint, Google F3) | Claude | Codex |
| F1, F2, F4, F6 | Assign in implementation plan | Other collaborator |
| Rehearsal and release verification | Shared with user | Both |

## 8. Shared work board

Statuses: **Planning**, **Ready**, **In progress**, **Needs review**, **Blocked**, **Done**. Use "Done" only with recorded evidence.

| Task | Description | Owner | Status | Evidence / next action |
| --- | --- | --- | --- | --- |
| PLAN-01 | Capture agreed scope and collaboration process | Codex | Done | This plan. |
| PLAN-02 | Review the plan; propose missing live-service requirements | Claude | Needs review | Spec revision 3 answers REVIEW-02 (R2-01..R2-04 + corrections). Codex to re-review §5.2, §5.4, §6.3, §6.4, §11, §19. |
| DESIGN-01 | Operator, Bible, remote flows | Claude | Needs review | Spec §7, §8, §12. Visual mockups not yet made. |
| DESIGN-02 | Architecture and shared interfaces | Shared | In progress | Tauri selected. Interfaces listed in §4 to define in the implementation plan. |
| REVIEW-01 | Codex adversarial review of spec revision 1 | Codex | Done | `docs/superpowers/reviews/2026-10-29-rustypresenter-v1-codex-review.md` |
| REVIEW-02 | Codex re-review of spec revision 2 and this plan | Codex | Done | Changes requested: 3 P1 and 1 P2 remain. See `docs/superpowers/reviews/2026-10-29-rustypresenter-v1-codex-review-rev2.md`. |
| BUILD-01 | v0.2 implementation: live engine, operator/room/stage/phone, songs, KJV, decks, media conversion, backups | Claude | Needs review | Windows build runs; 27 core unit tests pass on Windows. Windows runtime partly verified; see spec §19. |
| F1–F6, FEAS-01 | Feasibility checks (§3) | See §3 | Planning | October 29: user chose Tauri 2 over the v0.2 Edge app windows. F1 checked on this PC: room window has its own WebView2 data folder and browser process; a killed room renderer is reloaded in about 8 s. |
| AUDIT-01 | Decide what to reuse from the early experiment | Claude | Done | Rewritten. Reused ideas: SongSelect parsing, KJV source (book codes fixed: all 31,102 verses load). Old code kept in `experiment-archive/`. |
| NATIVE-P1 | Plan 1 foundation: git + CI, checks C1–C5, `App` out of HTTP, typed updates, Rust slide compositor + cache | Claude | Done (Windows) | Branch `native-foundation`. Release build and `cargo test` on Windows (see handoff Nov 1). C1/C2/C5 on macOS/Linux, C3 and the second-monitor case still need real machines or a person. |
| NATIVE-P2 | Plan 2: `video` module (libmpv), room output window, stage window | Claude | Done (Windows) | `docs/superpowers/plans/2026-11-01-native-video-room-stage.md`. Branch `native-foundation`, binary `rustypresenter-native`. Release build, `cargo test` and the libmpv-gated video tests pass on Windows; driven check of the native room/stage recorded in the checks doc (see handoff Nov 1, Plan 2). CI not yet run on the branch (not pushed). Monitor unplug, macOS/Linux video and a by-eye look still need real machines or a person. |
| NATIVE-P3 | Plan 3: operator window and dialogs | Claude | Ready | `docs/superpowers/plans/2026-11-01-native-operator.md`. |
| NATIVE-P4 | Plan 4: LibreOffice/pdfium import, supervisor + crash recovery, packaging for three platforms, remove Tauri | Claude | Planning | `docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md`; after Plan 3. |

## 9. Current workspace handoff

Codex began experimenting before the user clarified that this conversation was planning only. The files remain in place for inspection, but implementation has stopped.

- `Cargo.toml` / `Cargo.lock`: Rust package with Axum, Tokio, Serde, UUID, dirs.
- `src/model.rs`: service/slide structures, live snapshots, commands, JSON persistence.
- `src/server.rs`: local HTTP host, auth, pairing, commands, TXT conversion, KJV endpoints.
- `src/main.rs`: experimental host that opens a browser operator UI.
- `src/songselect.rs`: lyric TXT parser and slide splitting. Check its footer detection against the writers-before-`CCLI Song #` order.
- `src/bible.rs`: experimental KJV parser. **Known failing tests** (27,691 of 31,102 verses; legacy book IDs such as `JOH`, `1JO`, `EZE`, `MAR` unmapped; audit every mapping; source includes deuterocanon).
- `web/`: unfinished operator, remote, output pages.
- `assets/bibles/`: KJV source text and source info.
- `.tools/`, `target/`: local toolchain and build output; git-ignored.
- Last recorded: Rust compiled on Windows (GNU); tests 9 passed, 3 failed (all KJV). No end-to-end, phone, projector, macOS, or Linux verification.

The experiment predates the Tauri decision. Do not build on it without AUDIT-01.

## 10. Open product decisions

Resolved on October 29: displays (room + optional foldback), remote form (browser), media scope (backgrounds, video with audio, images, audio items), existing material (EasyWorship songs only), deck fidelity (images first), navigation at item end, Bible release gate.

Still open:

1. CCLI partner access for direct login (long-term).
2. Bible source choices and costs (F5); any translation exception is the user's decision.
3. Service planning integration (e.g., Planning Center): not requested; local service plans only for v1.

## 11. Research starting points

Links consulted October 29, 2026. Recheck before implementation.

- [CCLI partner service status](https://status.ccli.com/)
- [CCLI contact](https://songselect.ccli.com/about/contact)
- [ProPresenter's SongSelect workflow](https://support.renewedvision.com/hc/en-us/articles/360041815033-SongSelect-and-ProPresenter)
- [ProPresenter Clear Groups](https://support.renewedvision.com/hc/en-us/articles/4408681880211-Using-Clear-Groups-in-ProPresenter)
- [KJV source at eBible.org](https://www.ebible.org/find/details.php?id=eng-kjv)
- [API.Bible documentation](https://docs.api.bible/)
- [Crossway ESV API](https://api.esv.org/docs/) and [conditions](https://www.esv.org/api/)
- [Tyndale NLT API](https://api.nlt.to/)
- [WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)
- [W3C Audio Output Devices API](https://www.w3.org/TR/audio-output/)
- [Google OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app) and [policies](https://developers.google.com/identity/protocols/oauth2/policies)
- [Google Picker overview](https://developers.google.com/workspace/drive/picker/guides/overview)
- [SQLite backup API](https://www.sqlite.org/backup.html) and [WAL](https://www.sqlite.org/wal.html)

## 12. Handoff log

### October 29, 2026 — Codex

- Captured the confirmed requirements, including the absolute SongSelect requirement, six Bible translations, easy live Bible lookup, and PowerPoint/Google Slides imports.
- Recorded known prototype limitations and the failed KJV checks.
- Proposed a collaboration split and work board for Claude and Codex.
- Current action is planning only. No implementation work is assigned or running.

For the next handoff, append the collaborator name, task IDs, decisions made, files changed, evidence gathered, and the next unresolved step.

### October 29, 2026 — Claude

- Tasks: PLAN-02, DESIGN-01, part of DESIGN-02.
- Ran a brainstorming review with the user. Decisions are recorded in §0.
- Files added: `docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md`. Files changed: `PROJECT_PLAN.md` (§0, work board, this log). Backup of the previous plan: `PROJECT_PLAN.md.bak-2026-10-29`.
- Checked ProPresenter clear behavior against the Renewed Vision support article on Clear Groups. Whether Clear Media also stops a video's embedded audio was not stated there; this is flagged to verify.
- Not done: no code changed; no git repo exists yet (recommended as build step 1, along with moving the working copy out of OneDrive).
- Next: Codex reviews the design spec. After the user approves the written spec, write the implementation plan.

### October 29, 2026 — Codex adversarial spec review

- Reviewed PLAN-02, DESIGN-01, and the current DESIGN-02 direction against the new v1 spec and the active plan.
- Review artifact: `docs/superpowers/reviews/2026-10-29-rustypresenter-v1-codex-review.md`.
- Disposition: **changes requested before written-spec approval**. Seven P1 findings and five P2 findings; the review explains required revisions and acceptance checks.
- Accepted the recorded user scope decisions: Windows/Tauri v1, browser remote, foldback, video/audio, interim TXT/EasyWorship song imports, and image-based deck imports. The review does not reinstate direct SongSelect login as a v1 gate.
- Primary issues: unsupported renderer-isolation guarantee; missing requested-versus-actual output/media state; ambiguous input/navigation/reflow and clear semantics; missing required-translation acceptance gate; incomplete remote pairing/session policy; and active-plan contradictions with the new decisions.
- Checked relevant Microsoft, Google, W3C, and SQLite primary documentation. No code changed and no application tests were run during this review.
- The spec and task statuses were left unchanged for Claude's revision. Next: Claude addresses the review, reconciles the active plan, then returns the written changes for re-review before implementation planning.

### October 29, 2026 — Claude (spec revision 2)

- Tasks: PLAN-02, DESIGN-01, DESIGN-02; responds to REVIEW-01.
- Evaluated all Codex findings A01–A12 plus the additional notes; accepted all. No pushback.
- New user decisions: Next at last slide takes the next service item; v1 Bible release requires all six translations unless the user approves an exception; audio items added and Clear Audio kept; foldback optional.
- Files changed: spec rewritten as revision 2 (backup `docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.rev1.bak.md`); `PROJECT_PLAN.md` §0–§11 rewritten so the active plan matches v1 (backup `PROJECT_PLAN.md.bak-2026-10-29-pre-rev2`). Long-term goals (direct SongSelect login, macOS/Linux) kept as long-term rows.
- Spec §18 maps each finding to the section that addresses it. Platform claims (F1, F2, F3, F6) are designs pending feasibility checks, not verified behavior.
- No code changed. No tests run.
- Next: Codex re-review (REVIEW-02). Then user approval of the written spec, then the implementation plan.

### October 29, 2026 — Codex re-review of spec revision 2

- Completed REVIEW-02. Review artifact: `docs/superpowers/reviews/2026-10-29-rustypresenter-v1-codex-review-rev2.md`.
- Disposition: **changes requested; written spec not yet approved**. Nine original findings are resolved at design level. A02, A04, and A10 remain partially open, producing three P1 findings and one P2 finding.
- Remaining issues: global generation filtering discards valid events from retained/committed media; command identity and cue-only stale guards do not define safe retries/pending commands; item transitions do not explicitly stop a replaced video's audio when taking a deck live; backups specify hashes but not the required asset payload.
- Also recorded narrow corrections to navigation acceptance wording, explicit approval for any F1 fallback, the plan's error summary, and optional-foldback acceptance.
- Accepted the latest product decisions and the F1–F6 feasibility-check approach. These checks have not run and are not claimed as proven.
- Changes made only to the review artifact, REVIEW-02's work-board record, and this handoff. The design spec and application code remain unchanged. No application tests were run.
- Next: Claude revises the four remaining contracts and the consistency wording, then returns the written spec for targeted re-review before implementation planning.

### October 29, 2026 — Claude (spec revision 3 + v0.2 build)

- Tasks: PLAN-02 (answers REVIEW-02), BUILD-01, AUDIT-01.
- The user approved the design and asked for the app to be built and to be "very easy to use". The user also asked that video playback support practically any format.
- Spec revision 3 (backup `…design.rev2.bak.md`): §5.2 versions + playback instances (R2-01), §5.4 command IDs and base versions (R2-02), §6.3 incoming-item layer table (R2-03), §11 complete backups with asset bytes (R2-04), §4.1/§7.2/§14.2 wording corrections, §6.4 any-video via FFmpeg conversion, §18b response table, §19 implementation status.
- **Architecture changes** (spec §19): the user chose **Tauri 2** (October 29), replacing the v0.2 Edge app windows. Convert-on-import instead of libmpv playback still needs the user's confirmation.
- Files: new `src/` (`live.rs`, `server.rs`, `store.rs`, `slides.rs`, `bible.rs`, `songselect.rs`, `media.rs`, `displays.rs`, `sha256.rs`, `model.rs`, `main.rs`), new `web/` (operator, room, stage, phone, renderer, QR), `README.md`, `RustyPresenter.cmd`, `dev-build.cmd`, `dev-watch.cmd`. The previous `src/`, `web/` and `Cargo.toml` are in `experiment-archive/`. Backup of this plan: `PROJECT_PLAN.md.bak-2026-10-29-pre-rev3`.
- Evidence: 27 unit tests pass on Linux, including the review acceptance cases for R2-01..R2-04 and a full KJV load. The UI was checked in a headless browser against a stand-in API (operator, room, stage, phone, dialogs). **Not yet verified: the Windows build, the real server, PowerPoint export, monitor detection, FFmpeg conversion, phone pairing on a real phone.**
- Next: Windows build and fixes. Codex reviews revision 3 and the v0.2 code. Rehearsal on the church laptop.

### October 30, 2026 — Claude (scheduled continuation)

- **State found:** the code was switched back to **Tauri 2** after the last recorded build (`build.log` is from Oct 29 20:34, *before* `Cargo.toml`, `build.rs`, `tauri.conf.json`, `src/main.rs`, `src/displays.rs`, `src/server.rs` changed at ~01:28–01:38). So the Tauri version has **never been built or tested**. Spec §19 and the work board still describe the Edge-window design and are stale.
- **Tried:** compile-checking the Tauri code in the Linux cloud container; WebKitGTK dev packages are not installable there, so no result. Read-through of `open_window`, `watchdog`, and `main.rs` found no obvious errors, but that is not verification.
- **Next action for the user:** run `dev-build.cmd` on the church PC and read `build.log`; then `RustyPresenter.cmd`. Fix any compile errors before anything else.
- **Then:** update spec §19 / F1 to the Tauri design (separate WebView2 data folder per window), and run the Windows runtime checks listed in §19 "Verified".
- Stray empty file `.rp-src.tgz` was left in this folder by a temp step; safe to delete.
- **Superseded the same night (Claude, on the church PC):** the Tauri version was then built with `dev-build.cmd` (`BUILD EXIT 0`, 27 tests pass) and run: operator and room windows open, each in its own WebView2 process; a killed room renderer reloads in about 8 s; a second launch focuses the running app; closing the operator window quits everything. Spec §19 and F1 now describe the Tauri design. `sha2` and `qrcode` crates replaced the built-in stand-ins (spec §19 item 4). A smoke test on a throwaway data folder passed: Bible verse → live → room page, phone pairing with QR + PIN, remote Clear All. FFmpeg 9.0.2 installed via winget; AVI transcode, MOV remux and room playback verified. Still to check: full screen on a real second monitor, PowerPoint export, pairing from a real phone. The build's one linker warning (`.rsrc merge failure: multiple non-default manifests`) is expected; see `build.rs`.

### October 30, 2026 (late) — Claude (scheduled continuation 2)

- Rebuilt on the church PC through `dev-watch.cmd` (`build.request`): `BUILD EXIT 0`, 27 tests pass. No code changed this run.
- Still needs hands on the PC or a phone, so not done here: full screen on a real second monitor, PowerPoint export, pairing from a real phone, Sunday rehearsal.
- Codex still owes a review of spec revision 3 and the v0.2 code.
- `.rp-src.tgz` is a stray empty file; safe to delete.

### November 1, 2026 — Claude (native Plan 1 finished)

- Tasks: NATIVE-P1 done on Windows; NATIVE-P2 planned.
- Found two review-fix branches from an earlier run that had not been merged (`worktree-wf_b135fd26-fd1-2`: CSS-style color parsing, empty/zero theme values like `render.js`, bundled-only fonts for reference images, faster local shadow blur, script coverage tests; `worktree-wf_b135fd26-fd1-3`: SSE updates serialized once, exact Linux CI libraries). Merged both into `native-foundation`.
- Recorded checks C1–C5 above. Architecture and §0 now describe the native Slint decision; the Tauri rows are kept as history.
- Wrote Plan 2 (video, room output, stage) from the C1 results: `docs/superpowers/plans/2026-11-01-native-video-room-stage.md`.
- Still needs hands: second monitor on the church laptop, C3 drag-and-drop, macOS/Linux runs of C1/C2/C5.
- Next: carry out Plan 2.

### November 1, 2026 — Claude (native Plan 2 finished)

- Tasks: NATIVE-P2 done on Windows; NATIVE-P3 ready to start.
- Built on branch `native-foundation` (Tasks 1–8 committed with a review-fix commit after most): `src/video/` (libmpv loaded at run time, lookup `RP_LIBMPV` → exe dir → `<data>/deps/mpv/` → system names; players reporting by instance; `VideoSurface` drawing mpv under Slint with OpenGL or software), `src/room.rs` (which layers change and when a version is committed, like `web/output.js`), `src/platform/monitors.rs` (monitor ids, choice, unplug watch), `ui/room.slint` + `src/ui/room.rs`, `ui/stage.slint` + `src/ui/stage.rs`, `src/ui/screens.rs`, and the second binary `rustypresenter-native` (`src/bin/native.rs`) that runs the same `App` and phone server and serves the web operator page until Plan 3. The Tauri app still builds and runs.
- Evidence (this PC): `cargo test --locked --release` 135 + 1 + 10 passed; the same with `RP_VIDEO_TESTS=1` and a real libmpv-2.dll (client API 2.x) also passes; `cargo build --locked --release --bins --examples` builds both binaries. C1 rebuilt on `video::surface`: 29.99 (OpenGL) and 29.91–29.96 (software) distinct frames/s for 1080p30 with Slint text stable over the video. Native binary driven through the operator/phone HTTP API: 27 of 27 steps (slides, scripture, still, background and foreground video, pause, audio, clears, logo, phone remote, full screen on the one monitor). Without libmpv, slides work and media items report "The video player is missing. Reinstall RustyPresenter." Details in `docs/superpowers/checks/2026-native-checks.md`.
- CI (`.github/workflows/ci.yml`): Linux adds `libfontconfig-dev` (Slint's text layout probes fontconfig at build time; wayland, xkbcommon and GL are loaded at run time, so their `-dev` packages are not needed); every OS now also builds `--bins --examples`. Not pushed in this run, so CI has **not** run on these changes yet; gated video tests are skipped in CI (no libmpv there).
- Still needs hands: picture and sound checked by eye/ear on a projector, monitor unplug/replug on the church laptop, macOS/Linux runs of video and monitors.
- Next: push `native-foundation` and confirm CI on all three OSes; then carry out Plan 3 (operator window).
