# RustyPresenter v1 — Codex re-review of revision 2

**Date:** October 29, 2026
**Task:** REVIEW-02
**Reviewed:** Design spec revision 2 and the reconciled `PROJECT_PLAN.md`
**Disposition:** Changes requested. The written spec is not yet approved for implementation planning.

Revision 2 resolves nine of the twelve original findings at the design level. Feasibility checks F1–F6 remain explicitly unproven; their inclusion is a design resolution, not evidence that the platform capabilities work. Three original findings remain partially open, producing the four findings below: **three P1 and one P2**.

This review accepts the latest recorded product decisions, including Next crossing item boundaries, optional foldback, independent audio items, and the six-translation release gate. No code, runtime tests, or feasibility experiments were run. The spec was left unchanged.

Line references below refer to revision 2 as reviewed.

## Remaining findings

### R2-01 — [P1] The global generation rule discards reports from media that is still on air

**Location:** spec §5.2–§5.3, lines 91–99; persistent layers in §6.3, lines 142–144.
**Original finding:** A02, partially resolved.

The spec says to ignore **any** message from a generation older than the current one, while also requiring the previous committed output to keep playing during preparation and after preparation failure. These rules conflict.

**Concrete case:**

1. Video A is committed and playing at generation 20.
2. The operator requests B, creating generation 21. B is still preparing, so A remains on air.
3. A reports its playback position, ends, or encounters a playback error using generation 20. The blanket rule discards that report even though A is still the actual live media.
4. B times out. A remains the committed output, but its reports are still older than generation 21. The operator can now show stale time or a playing state after A has stopped.

This also affects an independent audio item that intentionally survives a slide change. The existing rule successfully rejects stale preparation completions, but it must not reject runtime events from retained live layers.

**Required revision:** Distinguish the newest transition/request identity from the committed frame and active media-instance identities. Runtime reports remain valid while their media instance is part of the committed/retained output, including while a newer request prepares or fails. Retired instances and canceled preparation completions must remain invalid. Define how a failed request exits the pending state without reviving or relabeling a retired player. Exact type names can wait for the build plan; these event-validity rules cannot.

**Acceptance:** Delay and then fail B while A plays. A's position, end, and failure reports remain authoritative. Clear or replace A, then deliver an old A completion: it does not restart playback or change the new output. Independently playing audio keeps reporting correctly across visual cue changes.

### R2-02 — [P1] Cue-only command targeting cannot distinguish stale commands from valid retriggers

**Location:** spec §5.4, lines 103–106; §6.3, line 143; clear-position behavior in §7.2, line 173.
**Original finding:** A02, partially resolved.

The relative-command guard contains only the visible cue ID. Clear All deliberately keeps that cue position, so an old command can still match after a newer clear. The spec also declares absolute commands idempotent, while an intentional retrigger of the same video/audio cue must restart playback. There is no command identity to distinguish one command delivered twice from two deliberate user actions.

**Concrete cases:**

- A phone sends Next from cue A; delivery is delayed. The operator issues Clear All, which keeps cue A. The old Next then arrives, still matches A, and brings the next cue live even though it was issued before the clear.
- Two Next commands naming A arrive while the first transition is still pending. If “live cue” means the committed cue, both can pass. The serialized queue alone does not establish the promised double-tap behavior; the spec must say when the expected state is consumed.
- A repeated delivery of TakeLive for the currently playing video is either ignored, violating the unconditional retrigger rule, or restarts playback, violating the idempotency rule. The protocol needs to distinguish delivery duplication from a new intentional trigger.

**Required revision:** Give commands a unique request identity for deduplication and an explicit base state/version where stale-state protection is required. Validate/consume relative-command preconditions atomically at admission, with defined behavior while a transition is pending. State that repeated delivery of the **same request** has no additional effect, while a **new request** to retrigger the same cue restarts it. Specify which commands, such as intentional global clears, may apply without a matching base version. Discarding unsent commands on reconnect remains appropriate but does not solve commands already in transit.

**Acceptance:** A Next issued before Clear All cannot undo that clear after delayed delivery. Two simultaneous Next requests based on the same state advance once. A duplicate TakeLive request does not restart media; a fresh intentional retrigger does, exactly once.

### R2-03 — [P1] Taking a deck live does not explicitly stop the previous video's embedded audio

**Location:** spec §6.3, lines 142–146; layer definitions in §6.1, lines 117–121.
**Original finding:** A04, partially resolved.

The new clear matrix defines individual clear buttons. The item-transition rule is still incomplete: a different item replaces the **slide and background** layers, and a **text item** stops foreground video. A deck image is expressly a Slide-layer image, not a text item, while foreground video occupies the separate Media layer.

**Concrete case:** Video A with embedded audio is playing. Next takes PowerPoint deck B live. An implementation following the stated rule replaces the Slide/background layers, displays B over A, and leaves A's foreground-video audio running underneath the sermon slides. The spec has no explicit rule forbidding this. Similar questions remain for a foreground still image followed by a song, or an audio item going live while visual content is displayed.

**Required revision:** Define the incoming-item layer policy for every v1 item kind. For each, identify what is replaced, cleared, retained, or restarted. Explicitly distinguish independent audio items, which are allowed to continue, from the embedded audio of the foreground video being replaced. Specify whether taking an audio-only item changes existing visuals. Keep the user-approved clear matrix; add the missing item-transition contract rather than assuming that table covers it.

**Acceptance:** Video with embedded audio → deck; video → foreground image; foreground image → song; and audio-only trigger during existing visual content all have one documented result. A replaced video's audio cannot continue accidentally behind another visual item; independent audio persists only according to the explicit policy.

### R2-04 — [P2] A database backup plus an asset manifest still does not contain the asset bytes

**Location:** spec §11, lines 278–281.
**Original finding:** A10, partially resolved.

Content-addressed assets and live-reference retention fix the overwrite/deletion problem. Backup contents are still described as a SQLite backup **plus a manifest of asset hashes**. A manifest identifies missing assets; it cannot reconstruct them. Keeping assets referenced in the original library also does not make a restore work when only the backup set is available. The separate service ZIP export includes assets, but the normal backup procedure does not say it does.

**Required revision:** State where the referenced asset bytes reside in a complete backup set: for example, within the backup bundle or in a dedicated, retained, deduplicated backup asset store. Derive the manifest from the same database snapshot and publish a backup as complete only when all required assets are available. Define restore using that backup set without relying on the original working library. If these are intended only as local database checkpoints, label them accordingly and distinguish them from the full restore promise.

**Acceptance:** Make a backup containing a video and imported deck. Restore into an empty data folder with the original working asset store unavailable. Both present successfully using only the selected backup set. A partially written backup is not offered as a complete restore point.

## Small consistency corrections

These should be corrected with the revisions above; they do not require reopening product decisions.

1. **Navigation acceptance, spec line 332:** “Song A live, B previewed: arrows never take B” is too absolute now that Next crosses item boundaries. Say that preview selection does not affect navigation: mid-item Next remains within A; at A's end it takes the actual next service item, including B if B is next in service order.
2. **F1 fallback, spec lines 35, 67, and 329:** §3 requires stopping and bringing options to the user when a feasibility check fails; §4.1 says the user is merely told and guarantees are downgraded, while acceptance still demands full isolation. Require an explicit recorded user decision for any reduced-isolation fallback, with matching acceptance criteria. Do not silently downgrade the agreed design.
3. **Plan summary, PROJECT_PLAN §2:** “errors never change the committed room output” remains broader than spec §5.3, which correctly clears a failed already-playing media layer. Use the spec's distinction between preparation failure, active-playback failure, and process recovery. Three-display acceptance should also be conditional on optional foldback hardware being available; the two-display configuration must have its own passing path.

## Original-finding disposition

| Original finding | Revision 2 disposition |
| --- | --- |
| A01 renderer isolation | Resolved as a conditional design and F1 check; apply the small fallback consistency correction above. Not hardware/runtime-verified. |
| A02 requested/actual output and commands | **Partially resolved:** acknowledgment and preparation design added; R2-01 and R2-02 remain. |
| A03 navigation/focus/reflow | Resolved at design level; apply the small navigation-acceptance wording correction above. |
| A04 clears/retriggering | **Partially resolved:** clear matrix added; incoming-item policy still needs R2-03. |
| A05 translation release gate | Resolved in §8.5 and the plan's per-translation table. F5 evidence still required. |
| A06 pairing/session policy | Resolved for the explicitly accepted trusted-LAN model. Implementation security checks remain required. |
| A07 plan contradictions | Resolved for platform, architecture, song import, remote, clear controls, and release scope. Correct the narrow error-summary wording above. |
| A08 audio routing | Resolved as a required F2 feasibility check with explicit device-loss behavior. Not proven. |
| A09 Google authentication | Resolved as a system-browser design with F3 checking the actual integration. Not proven. |
| A10 assets/backups | **Partially resolved:** immutable assets and retention added; complete backup payload needs R2-04. |
| A11 monitor roles | Resolved as a role/hot-plug design with F6 and optional foldback. Not proven. |
| A12 Bible layout/identity | Resolved at design-contract level; implementation and real-output tests remain required. |

## Next handoff

Claude should revise the event-validity rules, command identity/preconditions, incoming-item layer policy, and backup contents, then apply the small consistency corrections. Return the written changes for targeted re-review. Keep F1–F6 as early build checks rather than claiming they have passed. No implementation-plan approval or application implementation follows from this review.
