# RustyPresenter v1 — Codex adversarial design review

**Date:** October 29, 2026
**Reviewer:** Codex
**Reviewed:** `docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md` and `PROJECT_PLAN.md`
**Disposition:** Changes requested before written-spec approval. No code changes or implementation work authorized by this review.

The latest recorded user decisions are accepted: Windows-only v1, Tauri, a browser phone remote, foldback, video/audio, SongSelect TXT plus EasyWorship migration as the interim song workflow, and presentation imports as images. This review does not ask to reopen those decisions or reinstate the earlier direct-login gate for v1.

There are **7 P1 findings** and **5 P2 findings**. P1 findings affect live-service correctness, security, required scope, or the shared source of truth. P2 findings need a written behavior decision or an explicit early feasibility check before dependent implementation; they do not all require a prototype during planning.

Line references below refer to the files as reviewed, before any revisions.

## Findings

### A01 — [P1] Separate WebView2 windows do not guarantee separate renderer processes

**Location:** design spec line 51; related recovery promise at line 200.

The architecture asserts that each window gets its own renderer process and uses this to guarantee that an operator fault cannot take down output. Microsoft explicitly documents that one renderer can serve frames in multiple WebView2 instances sharing a user-data folder. Separate Tauri windows alone therefore do not establish this fault boundary. Distinguish an ordinary UI exception from a renderer hang/crash and shared browser/GPU process failure.

**Consequence:** A process-level failure during a service can affect both the operator and congregation display despite the stated isolation guarantee.

**Required revision:** Replace the asserted guarantee with an explicit resilience design. State which failures v1 isolates, how process failures are detected, how an affected output recovers, and what is shown during recovery. If dedicated process/environment separation is required, schedule a Tauri/WebView2 feasibility check before depending on it. Do not promise the impossible for shared GPU/runtime failures.

**Acceptance:** Inject an operator UI exception, renderer hang, and renderer crash separately. Record which windows fail and verify the specified recovery without unintentionally resuming audible media.

**Evidence:** [Microsoft WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model), [process-related events](https://learn.microsoft.com/microsoft-edge/webview2/concepts/process-related-events).

### A02 — [P1] Publishing desired state is not confirmation that output or playback succeeded

**Location:** design spec lines 85–95 and 198–202.

The bus updates core state and broadcasts a snapshot, but no renderer readiness/result channel or media playback authority is defined. Loading, decoding, starting, seeking, and changing an audio device can fail after a command is accepted. The operator could show a new live slide or a playing video while the room is still showing the old frame, black, or a failed player. A delayed play/seek completion can also arrive after Clear All and restart the cleared media.

**Required revision:** Describe the observable lifecycle: requested, preparing, ready/committed, playing/paused, failed. Identify the authoritative room media player and how it reports actual time, duration, playback state, and errors. Use cue/media generation identifiers so old completions cannot revive cleared/replaced content. Define command ordering, targeting, and duplicate/retry behavior for operator and phone commands. A reconnected phone should resynchronize state rather than replay stale presentation-changing commands.

Define the error promise precisely: preparation failure keeps the previous committed output; a failure of media already playing needs a specified recovery policy and cannot simply be described as never affecting output. Output rendering and clear commands must not wait behind network lookups, COM imports, or database backups.

**Acceptance:** Delay a media load, issue Clear All, then complete the old load: no video or audio restarts. Reject a play or seek request: the operator shows the real result. Run a deck import while advancing/clearing the presentation and verify the agreed responsiveness.

### A03 — [P1] Navigation, keyboard focus, and Bible reflow have conflicting or missing live boundaries

**Location:** design spec lines 65–68, 106–110, and 123–128.

Within a live item, slide clicks and arrow/space keys immediately change output; between items, Take Live is required. The spec does not define Next at the last slide, Prev at the first slide, or navigation while another item is selected in Preview. It also assigns Enter both to Take Live and to adding Bible results, and describes Bible slider edits as reflowing “live” despite the immutable LiveFrame rule.

**Consequence:** Two implementations can follow the prose yet behave differently at precisely the moments an operator is switching songs, typing a passage, or adjusting scripture already on air.

**Required revision:** Add a transition and input-context table. Cover no current live item, first/last slide, another preview item, cleared output, and selected/live items removed by service edits. Specify that typing into text/search/note fields or dialogs consumes its keys before presentation shortcuts. Clarify whether Bible reflow is preview-only, and when a revised item becomes the live navigation source. Define the resulting current/next cues for both phone and foldback. Repeated song-section occurrences need distinct cue identities even when they share lyric content.

**Acceptance:** With song A live and B previewed, arrows cannot accidentally take B. Enter in the Bible reference box performs exactly one documented Bible action. Reflowing an on-air scripture item does not change the current frame or silently skip/repeat verses during subsequent navigation.

### A04 — [P1] The clear controls lack a complete layer and retrigger contract

**Location:** design spec lines 61 and 70–83; video controls at lines 90–95; Logo button at line 180.

The layer model combines “Slide / Media item,” while Clear Slide and Clear Media are independent controls. A foreground still image/deck image, looping background video, foreground video, its embedded audio, and independent audio tracks are not mapped unambiguously. Clear Audio stops “audio tracks,” but standalone audio is not among the listed service item types. Logo is a named button with no defined action. Selecting an item is preview-only, but the retrigger rule says clicking an item brings cleared content back.

**Required revision:** Add a matrix identifying what each control does to each content type, including whether underlying backgrounds persist, what happens at video end, what happens when a different item goes live, and how layers return on a slide retrigger. Define the Logo action and audio support. Specify what supersedes/cancels an in-progress fade so an older transition cannot restore cleared layers. Treat “Clear Media stops its embedded audio” as the recorded v1 product decision unless the user changes it; matching a reference app should not substitute for defining our behavior.

**Acceptance:** For lyrics over a looping background, foreground video with embedded audio, a deck image, and any supported independent audio, verify each clear and subsequent slide/item trigger. Clear All during a fade must finish in the documented clear state.

### A05 — [P1] Required Bible translations are classified as nonblocking without a v1 acceptance gate

**Location:** design spec lines 132–135, 216, and 225–227; PROJECT_PLAN requirement R6.

All six translations remain required, but the spec puts five sources, terms, and costs under “Open items (do not block this design)” and builds them only “as each source is approved.” There is no distinction between a KJV-only development milestone and an acceptable v1 release, nor a decision if HCSB or another requested source cannot be obtained. This permits an implementation plan to silently end with five unavailable buttons.

**Required revision:** Keep KJV-first development if useful, but state the v1 completion criteria for each translation and whether full offline lookup or only prepared passages is required. Add an early source/access/cost check and an explicit user scope decision if a required source is unavailable. Define what Ready versus Offline-limited means for a newly requested passage when internet access fails. Do not call a required dependency universally nonblocking.

**Acceptance:** A readiness table records source, exact translation/edition, availability, attribution, permitted local storage, and representative lookup/preparation checks for NIV, ESV, CSB, HCSB, NLT, and KJV. An unavailable translation is not treated as satisfying R6.

### A06 — [P1] Four-digit remote pairing needs an abuse and session policy

**Location:** design spec lines 191–196; common command bus at lines 85–88.

A four-digit PIN has only 10,000 possibilities. The spec grants presentation control after PIN entry but defines no attempt limit, expiry, operator revocation, or command/session validation. With the proposed LAN server, a client on the same network could repeatedly guess until it gains the ability to clear output or start video. Typed command names alone do not enforce remote permissions.

**Required revision:** Define pairing attempt limits, a pairing window or expiry, PIN/session rotation, and how the operator revokes a phone. Use an explicit remote command allowlist, server-side authorization, and WebSocket origin checks. Keep trusted Tauri operator capabilities separate from the network remote. State the trusted-LAN/transport assumptions rather than implying that a PIN encrypts the connection. Put no long-lived privileged token directly in a QR URL without an explicit design for its use.

**Acceptance:** Repeated wrong PINs are throttled; a revoked token fails; a paired remote cannot invoke settings/import/edit/credential actions by constructing a request; reconnect does not bypass authorization or execute stale controls.

### A07 — [P1] PROJECT_PLAN still contradicts the newly approved v1 scope

**Location:** PROJECT_PLAN lines 42–52, 63–65, 108, 142–165, 174, 208, and 221–230; contrast with its new §0 and the spec's scope table.

The original body still makes direct SongSelect access a mandatory gate, requires all desktop platforms at acceptance, calls architecture unselected, leaves the phone form undecided, and requires Blackout/Restore. The new §0 selects the opposite v1 decisions. The spec even references the original R4 while deferring it. Both collaborators are instructed to use this plan as the shared record; neither precedence nor historical/superseded labeling makes the conflict safe to ignore.

**Required revision:** Update the requirements, workflows, feasibility gates, delivery phases, work board, and acceptance table to the current v1 decisions. Preserve long-term requirements in a clearly labeled later-scope section. State that the current v1 spec controls v1 behavior and link it explicitly; mark superseded material as historical or remove it from active instructions. Do not erase the user's long-term cross-platform/direct-login goals.

**Acceptance:** A collaborator reading only the active plan cannot conclude that v1 requires direct SongSelect login, Mac/Linux, or Blackout/Restore, or that Tauri is still undecided.

### A08 — [P2] Audio-device routing and unattended playback are assumptions that need an early check

**Location:** design spec lines 92–95 and build step 2 at line 214.

The spec commits to selectable audio devices “via WebView2 audio sink selection” without recording a proven path in the actual Tauri origin/runtime. The web audio-output API is permission- and secure-context-dependent; browser API existence alone does not prove device enumeration, routing, persistence across launches, or automatic playback from commands received by a separate output window. This is not a finding that WebView2 cannot do it; it is an unverified architectural dependency.

**Required revision:** Put an audio-routing/playback check ahead of dependent media implementation in the build plan. Verify the actual app origin, runtime, permission handling, and output player. Define behavior when the selected device disappears or changes identity: do not silently send audio to laptop speakers. Specify the fallback path or require a user decision if in-app routing cannot meet the requirement. Name the supported WebM codec combinations instead of promising every file with that container extension.

**Acceptance:** On the church laptop, route a real video to HDMI and the sound-board interface; restart the app, change the Windows default device, unplug/replug the selected output, and trigger playback from the phone without a click in the output window. Confirm a single audible player and truthful UI state.

**Evidence:** [W3C Audio Output Devices API](https://www.w3.org/TR/audio-output/) specifies secure-context exposure, permission failures, and device-loss behavior. This standard is not itself a WebView2 compatibility test.

### A09 — [P2] Google sign-in needs an external-browser desktop flow, not an implicit WebView login

**Location:** design spec lines 168–171 and 232.

The app is built from WebView2 windows, but “Google sign-in once” does not identify the authentication host or callback path. Google disallows authorization in developer-controlled embedded user agents. The proposed Picker also needs its supported hosting/origin and connection to the desktop authorization flow defined; choosing `drive.file` alone does not settle that integration.

**Required revision:** State system-browser desktop authorization, PKCE/state validation, a supported callback, protected refresh-token storage, and disconnect/re-consent behavior. Identify where Picker runs and how a chosen file returns to the app. Keep personal-project token expiry as an explicit verification task, and avoid promising sign-in literally only once. Preserve previously converted local slides if the account becomes unavailable.

**Acceptance:** From an installed app, authorize, select and export a private deck, restart and refresh access, revoke access, and reauthorize without exposing tokens to the phone or output pages.

**Evidence:** [Google OAuth policies](https://developers.google.com/identity/protocols/oauth2/policies), [desktop authorization flow](https://developers.google.com/identity/protocols/oauth2/native-app?hl=en), [Picker overview](https://developers.google.com/workspace/drive/picker/guides/overview).

### A10 — [P2] Immutable LiveFrame metadata does not keep referenced media or backups immutable

**Location:** design spec lines 57–60, 97–102, and 169–170.

A LiveFrame can retain an unchanged media reference while reimport overwrites the referenced file, library deletion removes it, or cleanup removes old images. Copying media into a library does not establish versioned identity or retention. Likewise, a SQLite/service snapshot alone cannot restore a service whose referenced media assets were lost or changed.

**Required revision:** Define immutable/versioned asset identities and retain every asset referenced by a committed live frame or retained service/backup. Reimport should create a new asset generation rather than overwrite live files. Describe backup contents, database/asset consistency, retention, and restore behavior. A restored service must resolve to the same required assets, not merely the same filenames.

**Acceptance:** Reimport and delete the library entry while its slide/video is live: active output remains valid. Restore a backup into a clean data folder and successfully present its media/deck assets. Interrupted import must not publish a half-complete deck.

### A11 — [P2] Display assignment has no missing-monitor or hot-plug behavior

**Location:** design spec lines 11, 22, 183–189, 202, and 230.

The third independent display is not yet confirmed, and there is no behavior for startup with a missing display, changing monitor identities, or an HDMI/display-adapter disconnect. Simply opening full-screen windows on chosen monitor indices is not enough to distinguish the room from foldback after a topology change. Foldback contains notes and stage messages that must not be placed on the congregation output accidentally.

**Required revision:** Define stable role assignment, startup confirmation/preflight, missing-output status, and safe handling of hot-plug/reordering. Specify whether v1 can operate without foldback while hardware is being acquired. Document how output windows recover without taking over the operator screen or assigning private foldback content to the room.

**Acceptance:** Connect the intended three-display setup, restart with one absent, unplug/reconnect each external display, and change scaling/primary display. Verify role assignments and clear operator alerts; confirm foldback content never appears on the room output because of an automatic reassignment.

### A12 — [P2] Bible pagination needs an output-layout and verse-identity contract

**Location:** design spec lines 111 and 117–128.

Fit-to-screen depends on the room's actual output dimensions, font metrics, line spacing, and reserved attribution area, not the size of the operator preview. The spec does not define that basis or what happens when an oversized verse has no sentence/clause boundary that fits. Manual breaks pinned only to a visual position cannot reliably survive text-size changes, translation changes, or split-verse continuations. Discontiguous inputs such as `Ps 23:1,4-6` also cannot be labeled simply `Psalm 23:1–6` without implying omitted verses are present.

**Required revision:** State the output geometry/font basis and deterministic fitting/fallback rules, including a minimum readable font size and word-boundary fallback if needed. Identify source verses and continuation ranges structurally, and pin manual breaks to stable source boundaries with defined behavior on translation changes. Preserve exact references for discontiguous selections and continuation slides. Do not require a complete implementation algorithm in the design; do establish its observable contract.

**Acceptance:** Test short and very long verses, discontinuous ranges, cross-chapter ranges, repeated reflow, missing fonts, and the target output resolutions/scaling. Verify no clipped attribution, omitted/reordered words, misleading range labels, or unexpected live changes.

## Additional requirements for the implementation plan

These supplement the findings and do not change scope:

- **Import isolation:** PowerPoint COM work, PDF rendering, large library imports, and network lookups must be cancellable/bounded and keep the presentation command path responsive. PowerPoint cleanup must affect only the application's own automation session, not the user's unrelated open presentations.
- **EasyWorship evidence:** Confirm the installed version and obtain a consistent, read-only copy before committing to its schema/RTF parser. Include duplicate handling, import rollback, and correct writer/footer extraction in fixtures. The observed SongSelect footer places the writer line before `CCLI Song #`; the early parser's footer detection must be checked against that ordering.
- **SQLite copies:** Define consistent backup/copy procedures for both migration fixtures and the new app. Copying an active database file alone can omit committed WAL transactions; prefer a supported backup operation or a verified quiescent source. [SQLite WAL documentation](https://www.sqlite.org/wal.html), [backup API](https://www.sqlite.org/backup.html).
- **Acceptance matrix:** Translate revised behavior tables into checks for simultaneous remote/operator actions, media failures, active imports, reconnect, and output/device recovery. Core unit tests plus one happy-path rehearsal are not sufficient evidence for these runtime boundaries.

## Recommended next handoff

1. Claude revises the design spec to address A01–A07 and records the decisions/checks required for A08–A12.
2. Claude reconciles PROJECT_PLAN's active requirements and acceptance criteria with §0 and the revised spec.
3. Codex re-reviews those written revisions. Only then record written-spec approval and proceed to the implementation plan when the user requests it.

No application tests were rerun for this documentation review. Platform-dependent findings above were checked against the linked primary documentation; actual Tauri/WebView2/church-hardware behavior remains untested.
