# FEAT-2 — Service Automation: Design Spec

**Date:** 2026-11-01 (revised after review the same day) · **Author:** Claude (Jake was asleep; every product choice is in §12)
**Status:** Design. Not built. Depends on FEAT-1 (overlay layer) and Plan 3 (native operator).
**Roadmap:** `PROJECT_PLAN.md` §10b, FEAT-2.
**Controls:** this spec adds to the v1 spec (`2026-10-29-rustypresenter-v1-design.md`) and the native spec (`2026-10-29-native-slint-design.md`). The live-safety rules of v1 §4.3 and §5–§7 still hold and win any conflict: **nothing visual goes live automatically unless the operator set it up and nobody has changed the screen since; an intentional clear always wins.**

## 1. Purpose and who it's for

A small church runs its service with one volunteer at the laptop. Today every action is a key press. This feature lets the app do the routine parts by itself, safely:

- **Timers** — "Service starts in 4:59" before the service, a sermon timer for the preacher, a stopwatch. Shown in the room and/or on the stage screen.
- **Messages with fill-in fields** — "Parent of child #**27**, please come to the nursery." The volunteer types only the number.
- **Macros** — one button runs several steps ("Walk-in": announcements loop + walk-in music + countdown).
- **Slide auto-advance and announcement loops** — a deck of announcements turns its own pages every 8 seconds and starts over.
- **Pre-service countdown** — a ready-made macro that sets all of the above up from one settings card.

Who: non-technical volunteers on old laptops. Everything works with zero setup (built-in timers and messages), is undone by the existing clears, and never surprises the room.

## 2. Scope

**In:**
1. Timers as core state ticking in `src` (not the UI): countdown to a time of day, countdown of a length, count up. Start / pause / reset / restart / set. Overrun behaviour. Shown as room overlays, on the stage screen, in the operator and on the phone.
2. Message templates with `{fields}`, shown as a room overlay banner; optional auto-hide.
3. Macros: a named list of steps built only from listed commands and actions (§5.3). Operator buttons; phone buttons for macros the operator marks "on phone".
4. Slide auto-advance per service item, with "start over after the last slide" (announcement loop).
5. Pre-service countdown preset. Service start time on each service.
6. Phone remote: macro buttons, show/hide message, timer readout.

**Out (later or other specs):**
- Crash recovery of automation state: goes into the same plan as native spec §8.3 (the supervisor and `live-state.json` don't exist yet). §5.10 gives the restore rule that plan must follow.
- Stage layouts and the stage message box (FEAT-3). FEAT-2 adds only an interim timer line on today's stage screen (§5.8).
- The overlay layer, `Frame.overlays`, overlay drawing, F6 Clear overlays (FEAT-1). FEAT-2 adds two overlay *sources* (timer, message).
- Which outputs show overlays (FEAT-7 screen views).
- Scrolling ticker messages, per-slide durations, loops across several items, time-based schedules, MIDI/hotkey triggers, waits inside macros, audio fades.

## 3. User experience

### 3.1 Where things are in the operator

- **Top bar:** a new **Automation** button (clock icon, tooltip "Timers, messages and macros") between Phone and Settings. It opens the Automation dialog with tabs **Timers · Messages · Macros · Pre-service**.
- **Live column**, under FEAT-1's Overlays strip, a new **automation strip**. The live column is 360–440 px wide (Plan 3), so the strip has a fixed layout of up to four rows, each row hidden when empty:
  - Row 1: **Message** button (flex width; tooltip "Show a message on the screen (Ctrl+Shift+M)"; while one is up it is accent-coloured and reads "Message showing — Hide") and, only while the live item auto-advances, the **Auto chip** (fixed 132 px): `Auto · 0:05` with ⏸/▶.
  - Row 2: **Timer chips**, fixed 168 px, at most 2 shown — those running, paused, finished or in the room, in Timers-tab order. If more qualify, the second slot becomes "+N timers" which opens a menu of the rest. A chip shows name, time (`4:59`, `+1:12` in red when over) and ▶/⏸. Click → menu: Start, Pause, Reset, Show in room / Hide from room, Set for now…
  - Rows 3–4: **Macro buttons**, fixed 112 px, three per row, at most 6 slots. With more than 6 macros, slot 6 is **More…** (a menu of the rest, in Macros-tab order). The pre-service macro is first when set up.
- **Slides grid:** a small clock badge on the first thumbnail of an item that auto-advances.
- **Item edit dialog:** an "Advance slides automatically" section (§3.5). **Services dialog:** a "Starts at" time field.

### 3.2 Timers — operator flow

Built-in timers (seeded once with fixed ids, editable, deletable):

| Name | Kind | Default | Overrun |
|---|---|---|---|
| Service start | Count down to a time of day | the current service's start time | Stop at 0:00 |
| Sermon | Count down a length | 30:00 | Keep counting (+m:ss) |
| Stopwatch | Count up | — | — |

Flow "sermon timer for the preacher": in Automation → Timers, "Sermon" has **Show on stage** ticked (default). When the preacher starts, the operator clicks ▶ on the Sermon chip (or a macro does it). The stage shows `29:59` counting down, then `+0:01` in red. Reset: chip menu → Reset → `30:00`, stopped.

Flow "shorter sermon this week": chip menu → **Set for now…** → prompt "Minutes for this time only" → `25`. Only this run changes; next week it is 30:00 again. Permanent changes are made in the Timers tab.

Flow "countdown in the room": chip menu → **Show in room** → the timer appears bottom-right over whatever is showing. **Hide from room** removes it. Showing is a live command (§5.2); ticking is not.

Timers tab: list with Name, Kind, Setting, Overrun, "Show on stage", "On phone", Edit / Delete; **New timer**. Edit form: Name; Kind (Segmented "Count down to a time" / "Count down" / "Count up"); for "to a time": "This service's start time" (default) or a fixed time (hour:minute picker, 12- or 24-hour per the computer's locale); for "Count down": minutes and seconds; Overrun ("Stop at 0:00" / "Keep counting"); When it reaches zero ("Do nothing" / "Hide it from the screen" / "Run a macro…" — only macros that show or hide things are listed, §5.4); Room position (Bottom right, Bottom left, Top right, Top left, Centre (large)); Caption (e.g. "Service starts in"); Show on stage; Show on phone.

### 3.3 Messages — operator flow

Built-in templates (seeded once): **Nursery** "Parent of child #{Number}, please come to the nursery.", **Vehicle** "Vehicle {Plate}: your lights are on.", **Custom** "{Message}".

Flow "nursery alert" (target under 10 seconds, even with a dialog open):
1. Press **Ctrl+Shift+M** (Cmd+Shift+M on a Mac) or click Message. Show message opens on the last template used this session (first template after a restart), focused on the first field. If Item edit, the Song editor or any other dialog is open, Show message opens as an ask-style overlay on top of it; the dialog underneath keeps what was typed.
2. Type `27`. The preview line shows the full sentence.
3. **Enter** (or **Show**). The overlay closes; a banner appears along the bottom of the room. Toast: "Message showing".
4. Done: click **Message showing — Hide**, or Ctrl+Shift+M → **Hide**.

One message at a time; showing another replaces it. "Hide after" on a template hides it by itself. Messages tab: list, **New message**, edit form: Name, Text (hint "Put a word in {curly brackets} to make a box to fill in, like {Number}."), Position (Bottom / Top), Hide after (Never / 15 s / 30 s / 1 min / 2 min / 5 min), "Can be shown from the phone".

In practice mode (FEAT-6) the dialog shows a yellow line: "Practice mode is on — this shows only in your preview. End practice to show it in the room."

### 3.4 Macros — operator flow

Macros tab: list (drag to reorder), **New macro**. Editor: Name, Colour (8 swatches), "Show on the phone remote", the step list with **Add step** and per-step up/down/remove. Steps (§4.3 has the types):

| Step | Editor wording | Parameters |
|---|---|---|
| ShowItem | "Put a service item on the screen" | "Item number N" or "First item called …"; slide number (shown 1-based, default 1) |
| Next / Prev | "Next slide" / "Previous slide" | — |
| Clear | "Clear …" | Everything / Words / Video and pictures / Audio / Overlays |
| Logo | "Show the logo" | — |
| Overlay | "Turn an overlay on / off" (FEAT-1 saved overlay) | overlay, on/off |
| ScreenView | "Change what a screen shows" (FEAT-7) | screen, screen view |
| TimerStart / Pause / Reset / Restart | "Start / Pause / Reset / Restart timer" | timer |
| TimerShow / TimerHide | "Show timer on the screen" / "Hide timer from the screen" | timer |
| MessageShow / MessageHide | "Show a message" / "Hide the message" | template + fixed values for every field |
| AutoAdvance | "Pause / resume automatic slides" | on/off |
| StageMessageShow / StageMessageHide | "Show a message to the stage" / "Hide the stage message" (FEAT-3) | text |
| StageLayout | "Change the stage layout" (FEAT-3) | layout |

Running: click the button. The screen part of a macro happens as one change (all or nothing); stage steps follow it (§5.3). Toast "Ran “Walk-in”". If the screen changed between the operator seeing it and clicking (an AutoNext excepted, §5.6), the macro does not run and the toast says "The screen changed — press the button again."

### 3.5 Slide auto-advance and announcement loops

Item edit, for Song, Scripture, Announcement and Slides items: **Advance slides automatically**, "every [8] seconds" (2–600), **Start over after the last slide** (default ticked). Items with one slide show the section disabled with "Needs at least two slides."

On air: slides turn every N seconds; with "start over" it loops; without, it stops on the last slide. It never moves into the next service item. The operator's Next/Back/clicks still work and restart the count; Next on the last slide takes the next item (v1 §7.2), ending the loop. Any clear or Logo stops it until the operator shows a slide of that item again.

### 3.6 Pre-service countdown

Services dialog: each service has "Starts at" (empty by default). The Pre-service tab is one card:
- **Countdown to:** this service's start time (read-only; "Set a start time in Services first" link when empty).
- **Loop these slides:** ComboBox of the service's auto-advancing items, plus "None". Saved by item title, so it works on next week's copy.
- **Play music:** the service's audio items, plus "None". Saved by title.
- **Countdown position:** Bottom right (default with a loop) / Centre large (default without).
- **Caption:** "Service starts in".
- **When the countdown ends:** "Hide the countdown" (default) / "Show the logo" / "Clear everything" / "Put the first item on the screen". Under the last three: "Only happens if nobody has changed the screen since the countdown started."
- **Save** creates or updates the built-in timer **Pre-service** and the locked macro **Start countdown** (first in the strip).

Sunday: at 10:00 the operator clicks **Start countdown**. Music starts, the announcements loop, the room shows "Service starts in 29:12". At 10:30 the countdown hides. With "Put the first item on the screen", item 1 goes live at 10:30 — but only if the screen is still the loop; if the operator already took the first song at 10:28, nothing changes except the countdown hiding, and the notice says "Countdown ended — nothing else changed because the service has started." Walk-in music keeps playing until Clear Audio, as today.

### 3.7 Phone remote

Below the clear buttons (and FEAT-1's overlay chips), sections appear only when they have content:
- **Timers:** a read-only line per timer marked "Show on phone": `Sermon 12:41`.
- **Message:** a button per template marked for the phone. Tap → fields → **Show**. While up: **Hide message**.
- **Macros:** a button per macro marked for the phone. Tap → confirm sheet "Run “Walk-in”?" → Run. It sends the phone's `base`; a stale result shows "The screen changed — try again." and the phone resyncs.

### 3.8 Room and stage effect

Room: timers and the message banner are sources on FEAT-1's overlay layer. **One layer order for the whole app (FEAT-1 §6.1 owns it):** background < media < slide < logo < overlays < fade-to-black. So a message and a countdown draw over the logo screen. Stage: interim timer line (§5.8) until FEAT-3.

## 4. Data model

Modules: `src/timers.rs` (timer definitions, state machine, `TimerSnapshot`, formatting — the path FEAT-3 codes against), `src/automation/{mod,runtime,messages,macros,advance,preservice,ticker,strings}.rs`, and a small shared `src/lenient.rs`. Persisted types are re-exported from `crate::model`.

**Time library: `chrono`** (already in `Cargo.toml`; FEAT-1, FEAT-3 and FEAT-6 use it). Wall clock instants are `chrono::DateTime<Utc>`; local time of day uses `chrono::Local`. Calculations are generic over `Tz: chrono::TimeZone` so tests can pin a zone with `chrono-tz` (a **dev-dependency only**). DST: a local time that doesn't exist (spring-forward gap) uses the first valid instant after it; an ambiguous one (fall-back) uses the earlier. One time crate for the app; `jiff` is not added.

### 4.1 Forward compatibility

`Store::load` fails the whole file on any parse error, so nothing FEAT-2 adds may fail it:

```rust
// src/lenient.rs — keeps data written by a newer RustyPresenter
pub enum Lenient<T> { Known(T), Unknown(serde_json::Value) }
// Deserialize: read a Value; T::deserialize(&value) → Known, else Unknown(value). Never fails.
// Serialize: Known → T, Unknown → the raw value unchanged (round-trips).
```
- `MacroStep`, `TimerKind` and `AtZero` are stored as `Lenient<…>`. An unknown step runs as a failing step: "This step needs a newer RustyPresenter." An unknown timer kind can't start ("This timer needs a newer RustyPresenter.") and is left out of `App::timers()`.
- Unit-only enums (`Overrun`, `OverlayPlace`, `MessagePlace`, `MacroColor`, `PreServiceEnd`, `Builtin`, `ClearWhich`) put `#[serde(other)]` on their default variant, so an unknown value loads as the default.
- `TimeOfDay` deserializes "H:MM"/"HH:MM", clamping hour to 0–23 and minute to 0–59 ("25:70" → 23:59). Unparsable text fails only that value: `Service.starts_at` uses `deserialize_with = "lenient_time"` → `None`; inside a `TimerKind` it makes the kind `Unknown`.
- Every struct has `#[serde(default)]` and a `Default` impl, `PreService` included.

### 4.2 Persisted (data.json)

```rust
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Automation {
    pub timers: Vec<TimerDef>,
    pub messages: Vec<MessageTemplate>,
    pub macros: Vec<Macro>,
    pub pre_service: Option<PreService>,
    pub seeded: bool,                       // built-ins created once; never re-created after deletion
}

#[serde(default)]
pub struct TimerDef {                       // src/timers.rs
    pub id: Uuid, pub name: String,
    pub kind: Lenient<TimerKind>,
    pub overrun: Overrun,                   // Stop (default) | CountUp
    pub at_zero: Lenient<AtZero>,
    pub place: OverlayPlace,                // BottomRight (default) | BottomLeft | TopRight | TopLeft | CenterLarge
    pub caption: String,
    pub on_stage: bool, pub on_remote: bool,
    pub builtin: Option<Builtin>,           // Some(PreService) for the pre-service timer
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimerKind { Duration { secs: u32 /* 1..=86_399 */ }, UntilTime { target: TimeTarget }, CountUp }
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimeTarget { ServiceStart, Fixed { at: TimeOfDay } }
pub struct TimeOfDay { pub hour: u8, pub minute: u8 }       // "HH:MM", lenient (§4.1)
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AtZero { Nothing, HideFromRoom, RunMacro { macro_id: Uuid } }  // default Nothing

#[serde(default)]
pub struct MessageTemplate { pub id: Uuid, pub name: String, pub text: String /* {Field}; {{ }} literal */,
                             pub place: MessagePlace /* Bottom | Top */, pub hide_after_secs: Option<u32>, pub remote: bool }

#[serde(default)]
pub struct Macro { pub id: Uuid, pub name: String, pub color: MacroColor /* Amber default + 7 */,
                   pub steps: Vec<Lenient<MacroStep>>, pub remote: bool, pub builtin: Option<Builtin> }

#[serde(tag = "step", rename_all = "snake_case")]
pub enum MacroStep {
    // Live steps (§5.3): applied inside one Live batch
    ShowItem { target: ItemTarget, #[serde(default)] slide: usize /* 0-based; the editor shows slide+1 */ },
    Next, Prev,
    Clear { which: ClearWhich },            // All | Slide | Media | Audio | Overlays
    Logo,
    Overlay { id: Uuid, on: bool },         // FEAT-1 saved overlay
    ScreenView { screen: OutputRole, view: Uuid },   // FEAT-7 SetScreenView
    TimerShow { timer: Uuid }, TimerHide { timer: Uuid },
    MessageShow { template: Uuid, values: BTreeMap<String, String> },
    MessageHide,
    // Runtime steps: change the automation runtime, inside the same checkpoint
    TimerStart { timer: Uuid }, TimerPause { timer: Uuid }, TimerReset { timer: Uuid }, TimerRestart { timer: Uuid },
    AutoAdvance { on: bool },
    // Action steps: do_action calls run after the batch commits (FEAT-3)
    StageMessageShow { text: String }, StageMessageHide, StageLayout { layout: Uuid },
}
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemTarget { Number { n: usize /* 1-based, as the volunteer counts */ }, Titled { title: String } }

#[derive(Default)] #[serde(default)]
pub struct PreService { pub loop_title: Option<String>, pub music_title: Option<String>, pub place: OverlayPlace,
                        pub caption: String /* "Service starts in" */, pub end: PreServiceEnd /* Hide | Logo | ClearAll | FirstItem */ }
```

**Never macro steps** (each needs a fresh human look or is operator-only): FEAT-4 `Jump` and `Refresh`, `MediaPause`/`AudioPause`, FEAT-6 `practice/start`/`stop`, any editing action, `RunMacro` itself (no nesting). New step kinds join only by being listed in `MacroStep` with a class (Live / Runtime / Action) in a later spec.

Changes to existing types (serde defaults; old `data.json` loads unchanged):
```rust
pub struct Data        { /* … */ #[serde(default)] pub automation: Automation }
pub struct Service     { /* … */ #[serde(default, deserialize_with = "lenient_time")] pub starts_at: Option<TimeOfDay> }
pub struct ServiceItem { /* … */ #[serde(default, skip_serializing_if = "Option::is_none")] pub advance: Option<AutoAdvance> }
pub struct AutoAdvance { pub secs: u16 /* 2..=600 */, #[serde(default = "yes")] pub loop_item: bool }
pub struct LiveItem    { /* … */ pub advance: Option<AutoAdvance> }   // snapshot at Take Live (v1 §7.2)
```

**Overlay contents FEAT-2 adds to FEAT-1's `OverlayContent`** (FEAT-1 owns the enum, `Frame.overlays: Vec<OverlayOn { id, z, overlay }>` and the mutators):
```rust
OverlayContent::Timer   { timer_id: Uuid, caption: String, large: bool, look: OverlayLook }  // bound text
OverlayContent::Message { shown: Uuid /* new per Show */, template_id: Uuid, text: String /* filled */, look: OverlayLook }
```
`OverlayOn.id` = the timer id, or the constant `MESSAGE_SLOT` (one message). `z` = `1_000_000 + timer index` for timers and `u32::MAX` for the message, so both sit above saved overlays and the message is on top. They don't count toward FEAT-1's 8-overlay limit; at most 4 timers can be on the screen ("Up to 4 timers can be on the screen at once."). The filled text is stored, so a message is not bound; a timer is bound (§6).

**Seeding:** if `seeded` is false, `automation::seed` adds the built-ins with **fixed ids** (`timers::SERVICE_START_ID`, `SERMON_ID`, `STOPWATCH_ID`, `messages::NURSERY_ID`, `VEHICLE_ID`, `CUSTOM_ID`, constants in code) and the store saves at once (`store.edit`, one write at first launch). `Data.format` stays as FEAT-1 sets it; nothing else migrates. Unknown ids in steps are kept and reported when run.

### 4.3 Runtime (memory only)

```rust
pub struct TimerRun { pub state: RunState, pub run: u64 /* bumps on reset/restart */, pub zero_fired: bool,
                      pub length_for_now: Option<u32> /* chip "Set for now…" */ }
pub enum RunState {
    Stopped,
    Running { since: DateTime<Utc>, carried_ms: u64 },        // Duration, CountUp
    RunningTo { target: DateTime<Utc> },                       // UntilTime: target fixed at start
    Paused { elapsed_ms: u64 },
    Finished,                                                  // countdown with Overrun::Stop reached zero
}
pub struct AutoRun { pub item_id: Uuid, pub slide: usize, pub armed_visual: u64, pub due: DateTime<Utc>,
                     pub waiting_for: Option<u64> /* version whose Committed restarts `due` */ }
pub struct PreRun { pub manual_version: u64 /* Live.manual_version right after Start countdown */, pub loop_item: Option<Uuid>, pub timer_run: u64 }
pub struct AutomationRuntime {
    pub timers: HashMap<Uuid, TimerRun>,
    pub auto: Option<AutoRun>, pub auto_paused_item: Option<Uuid>,
    pub hide_at: HashMap<Uuid /* message `shown` */, DateTime<Utc>>,
    pub pre: Option<PreRun>,
    pub last_message: Option<Uuid>,                            // template Show message opens on
}
```
Lives in `App.automation: Mutex<AutomationRuntime>`. **The frame is the single source of truth for what is on screen:** "message showing", "timer in room" and the shown message's id are always read from `Live.frame.overlays` of the active engine, never stored here.

### 4.4 Readouts (contract with FEAT-3)

```rust
pub struct TimerSnapshot { pub id: Uuid, pub name: String, pub kind: TimerKind,
                           pub state: TimerState /* Stopped | Running | Paused | Finished */,
                           pub value_ms: i64 /* remaining for countdowns, elapsed for count-up; negative = overrun */,
                           pub overrun: bool, pub on_stage: bool, pub on_remote: bool }
impl App { pub fn timers(&self) -> Vec<TimerSnapshot> }   // Timers-tab order, unknown kinds left out
pub struct TimersView { pub timers: Vec<TimerSnapshot>, pub auto: Option<AutoReadout { next_in_secs: u32, paused: bool }> }
Update::Timers(Arc<TimersView>)   // what FEAT-1 §5.5 calls the core tick
pub fn timers::text(value_ms: i64) -> String   // shared formatter
```
Formatting is one pure function used by everyone (operator, room, phone; FEAT-3's `view::timer_text` calls it and adds its own colours/captions). Countdowns round **up** (`0:01` until really zero); count-up rounds down; `m:ss` under an hour, `h:mm:ss` from an hour; overrun `+m:ss`. Stopped shows the start value (full length, the time left to the target, or `0:00`).

## 5. Core behaviour

### 5.1 Timer state machine

| From | start | pause | reset | restart | reaches zero |
|---|---|---|---|---|---|
| Stopped | Running / RunningTo | — | — | Running / RunningTo | — |
| Running | no-op | Paused | Stopped | Running(0) | Stop → Finished; CountUp → stays Running, `overrun` |
| RunningTo | no-op | refused | Stopped | RunningTo (new target) | Stop → Finished; CountUp → stays, `overrun` |
| Paused | Running(carried) | no-op | Stopped | Running(0) | — |
| Finished | Running / RunningTo | — | Stopped | Running / RunningTo | — |

- **UntilTime target is fixed at start/restart** and stored in `RunningTo`. It is never recomputed while running, so an overrunning timer keeps counting up (`+1:05:00`), and editing the service's `starts_at` or switching service doesn't move it (the Services dialog says "Restart the countdown to use the new time." while it runs).
- Computing the target at start: `ServiceStart` → today at the current service's `starts_at`; missing → "Set a start time for this service first (Services → Starts at)."; already past → refused, "The service start time (10:30) has passed." `Fixed{at}` → today at `at` if still ahead; if it passed less than 60 minutes ago → refused, "That time (10:30) has passed."; otherwise tomorrow at `at`. A refused start inside a macro fails the whole batch.
- Pause on `RunningTo` → "A countdown to a time can't be paused." (the UI hides the button).
- **Set for now** (`timer/set-now {id, secs | at}`) changes only this run (`length_for_now`, or a new target for RunningTo) and never the saved definition; Reset clears it. The Timers tab saves definitions.
- **At zero** fires once per run (`zero_fired`), when the ticker sees a running countdown cross zero. Reset/restart clears it.

### 5.2 Live engine changes (single source, batches, origin)

**Refactor of `src/live.rs`.** Today `go()` and every arm of `run()` call `bump()` inline, and `bump()` also pushes a history snapshot. This splits into mutation and commit:

```rust
enum Change { None, Overlay, Visual }       // what a mutation touched; Visual = slide/background/media/logo/audio/cursor
enum Origin { User, Auto }                  // Auto = InternalCommand::AutoNext only
fn mutate(&mut self, step: &LiveStep, plan: &dyn Plan) -> Result<Change, String>   // go(), clear arms, FEAT-1 overlay put/remove, screen view
fn commit_change(&mut self, change: Change, origin: Origin)                       // the only place that bumps
pub fn apply_batch(&mut self, env_id: &str, base: Option<u64>, steps: &[LiveStep], plan: &dyn Plan, origin: Origin) -> Outcome
```
- `go()` no longer bumps or clears `notice`; it still allocates instances. `commit_change`: `version += 1`; one `remember()` (one history snapshot); clears `notice` when Visual; sets `visual_version = version` when Visual; sets `manual_version = version` when Visual and `User`; maintains `auto_run_from` (below).
- `run()` for a single command = base check → `mutate` → `commit_change` (single commands keep today's "always bump" behaviour; FEAT-1's idempotent overlay toggles return `Change::None` and don't bump).
- `apply_batch`: (1) seen id → remembered outcome (one `seen` entry per batch); (2) base check once, if a base is given or required; (3) checkpoint `frame, cursor, item, next_inst, notice`; (4) `mutate` each step in order — later steps see earlier ones; (5) any error → restore the checkpoint, outcome `ok:false` "“Walk-in”: step N — <reason>", no bump, no broadcast; (6) else one `commit_change` with the strongest change (none → no bump).
- New `Live` fields: `visual_version`, `manual_version`, `auto_run_from: Option<u64>` (all `#[serde(default)]` where serialized).

**Base freshness with AutoNext.** `auto_run_from` is set to the pre-bump version on the first `Auto` commit after any `User` commit and cleared by every `User` commit. A base is fresh if `base == version`, or `auto_run_from == Some(s)` and `s <= base < version` (only automatic slide turns happened since). This applies to every base-checked command (Next, Prev, Show, FEAT-4 Jump, relative macros). So a press made just after an auto-advance is not lost (it moves on from the slide now showing), while anything after a Clear, Logo or other user change is still stale — v1 §5.4 holds.

Existing tests that must still pass unchanged: `delayed_next_cannot_undo_clear_all`, `two_nexts_from_same_state_advance_once`, every `report`/history test in `live.rs`. New: `batch_records_one_history_snapshot`, `batch_failure_restores_next_inst_and_notice`, `operator_next_right_after_auto_next_is_not_lost`, `next_after_clear_all_still_stale_despite_auto_next`.

### 5.3 Commands, macros and atomicity

New `live::Command` variants (deserializable; from operator, `/api` and the phone allowlist):
```rust
RunMacro    { macro_id: Uuid },          // envelope base required when the macro is relative
ShowMessage { template: Uuid, values: BTreeMap<String, String> },
HideMessage,
ShowTimer   { timer: Uuid }, HideTimer { timer: Uuid },
```
Show/Hide message/timer carry no base (absolute, like clears and FEAT-1 overlays). Internal work uses a separate **non-deserializable** enum, so no HTTP route can ever build it:
```rust
pub enum InternalCommand {
    AutoNext { item_id: Uuid, from_slide: usize, armed_visual: u64 },
    AutoHideMessage { shown: Uuid },
    TimerAtZero { timer: Uuid, run: u64 },
    PreServiceEnd { run: u64 },
}
impl App { pub fn command_internal(&self, c: InternalCommand) -> Outcome }   // fresh id "auto-<uuid>"
```
`App::command` resolves these before the engine (they need `Data` and the runtime). **Lock order for the whole app, documented in `app.rs`: `data` → `live` → `practice` → `automation`.**

**Relative macros.** `Macro::is_relative()` is true if any step is ShowItem, Next, Prev, Clear, Logo or ScreenView. A relative macro needs a fresh base (§5.2): the operator's button sends `base = live.version` from its last LiveUpdate; the phone always sends its base. Missing or stale → "stale: …"; the operator and phone show "The screen changed — press the button again." (not silent, unlike keys). So a phone macro sent before the operator's F1 can never bring the cleared content back. Non-relative macros (timers, messages, overlays, stage) skip the base check.

**Running a macro:**
1. Resolve every reference up front (timers, templates, overlays, layouts, item targets against the current service: `Number{n}` → item n, "There is no item 5 in this service."; `Titled` → first title equal ignoring case and outer spaces, "No item called “Announcements” in this service."). Unknown step → "This step needs a newer RustyPresenter." Any failure → nothing changes.
2. With the locks held: checkpoint the runtime, run Live and Runtime steps in order through `apply_batch` (Runtime steps are applied by the batch's step callback to the locked runtime). Any failure → engine and runtime restored. Success → one version bump, one room transition.
3. Then the Action steps, in order, through `do_action` (FEAT-3 `stage/message/show`, `stage/message/hide`, `stage/layout/use`). They are not rolled back; each failure is reported: toast "Ran “Walk-in”, but step 6 didn't work: That layout is gone." They don't touch the room, so the room-safety rules are unaffected.

A double-click with the same id runs once; a new click runs again.

### 5.4 Automatic triggers (at-zero) and the pre-service end

Nothing automatic may change the visual layers unless the operator set it up and nobody has changed the screen since:
- `AtZero::RunMacro` accepts only **overlay-only** macros: steps from TimerShow/TimerHide, MessageShow/MessageHide, Overlay, Timer* runtime steps, Stage* action steps. Saving a timer that points at another macro → "A timer can only run a macro that shows or hides things (timers, messages, overlays)." Editing a macro to become relative while a timer points at it → the same check runs at fire time; it doesn't run and the notice says "“Sermon” reached zero, but “X” changes slides, so it didn't run."
- **Pre-service end** is `InternalCommand::PreServiceEnd`, not a macro. At zero it always hides the countdown (overlay-only). If `end` is Logo / ClearAll / FirstItem, those steps join the same batch **only if** `runtime.pre` is set for this timer run and `live.manual_version == pre.manual_version` (no user visual change since Start countdown) and the cursor is on `pre.loop_item` or nothing visual is live. Otherwise only the hide happens and the notice says "Countdown ended — nothing else changed because the service has started." `pre` is cleared after it fires and by any reset/restart of the Pre-service timer.

`automation::preservice::build(&PreService) -> Vec<MacroStep>` (macro marked `builtin: PreService`, locked in the Macros tab: "Change it in the Pre-service tab"):
1. `ShowItem{Titled(music_title)}` if set — **music first**, because `go()` moves the cursor for audio items too (v1 navigation is unchanged by this spec);
2. `ShowItem{Titled(loop_title)}` if set — so the cursor ends on the loop item and auto-advance arms on it;
3. `TimerRestart{pre-service timer}`, `TimerShow{pre-service timer}`.
After the batch commits, `App` records `runtime.pre = PreRun{ manual_version: live.manual_version, loop_item, timer_run }`. The built-in timer is `UntilTime{ServiceStart}`, `Overrun::Stop`, `AtZero::Nothing` (the end is handled above), place and caption from the card. A start time already past → the restart fails, the whole macro doesn't run: "The service start time (10:30) has passed."

### 5.5 Clear matrix

Timers and messages follow FEAT-1's overlay row exactly (FEAT-1 holds the combined table):

| Content | Clear All F1 | Clear Slide F2 | Clear Media F3 | Clear Audio F4 | Logo F5 | Clear overlays F6 |
|---|---|---|---|---|---|---|
| Timer overlay | Removed | Stays | Stays | Stays | Stays | Removed |
| Message banner | Removed | Stays | Stays | Stays | Stays | Removed |

- Clears hide overlays only; timer **state** never changes (a hidden Sermon timer keeps counting on stage).
- Incoming items (v1 §6.3) leave overlays alone.
- Auto-advance stops on Clear All, Clear Slide, Logo, and on video/still items (no slide layer). It re-arms with a full interval when the operator shows a slide of an advancing item (§5.6).

### 5.6 Auto-advance

`automation::advance::reconcile(rt, live, now)` runs under the locks after every accepted command, internal command, and every `Live::report` that returns true (Committed and Failed included), on the **active engine** (FEAT-6 `App::with_active_engine`):
- If the live item has no `advance`, has fewer than 2 slides, or the slide layer is not up → `auto = None`.
- Else if `auto` is None or `auto.armed_visual != live.visual_version` → arm: `AutoRun{ item_id, slide: cursor.slide, armed_visual: visual_version, due: now + secs, waiting_for: Some(version) }`. Overlay-only changes don't change `visual_version`, so showing a message doesn't restart the count. A Failed revert changes `visual_version`, so auto re-arms from the reverted cursor (or stops).
- On `Committed(v)` with `waiting_for == Some(v)`: `due = now + secs`, `waiting_for = None` (slow preparation doesn't shorten a slide).
- At `due` (and not paused), the ticker sends `AutoNext{item_id, from_slide, armed_visual}`. Applied only if the cursor is exactly there, the slide layer is up and `visual_version == armed_visual`: next slide; or slide 0 on the last slide with `loop_item`; or `auto = None` on the last slide without loop. Never crosses items. `Origin::Auto`.
- Pause/resume: action `auto/set {on}` (and the AutoAdvance macro step) sets `auto_paused_item`; resuming re-arms with a full interval. Pausing ends when another item goes live.
- Uses the LiveItem snapshot (v1 §7.2): editing the interval takes effect next time the item is taken live.
- Stage and phone "next" keep showing what a manual Next would do (on a looping item's last slide that is the next service item, not slide 1).

### 5.7 Messages

- `ShowMessage`: look up the template; `messages::fill(text, values)` — every `{Field}` non-empty after trimming; ≤ 200 characters; newlines → spaces. Then FEAT-1's overlay put of `OverlayOn{ id: MESSAGE_SLOT, content: Message{ shown: new uuid, … } }`, replacing any current one; `hide_at[shown] = now + hide_after_secs` if set; `last_message = template`.
- `HideMessage` removes the slot (no-op, no bump if none). `AutoHideMessage{shown}` removes it only if the frame's message still has that `shown`.
- After every reconcile, `hide_at` entries whose `shown` is no longer in the frame are dropped (Clear All, F6, a Failed revert). The Message button and the phone's `message_shown` read the frame, so they are always right.
- Field parsing: `{`…`}` with 1–30 characters, no nesting; `{{`/`}}` literal; order of first appearance; duplicates share one box; a stray brace saves with the warning "This message has a { without a matching }." and shows literally.
- Practice mode (FEAT-6): message and timer commands go to the practice engine like every non-clear command; the room doesn't change.

### 5.8 The ticker and Update::Timers

- `automation::ticker(app: Arc<App>)` on the tokio runtime sleeps on a `Notify` while no timer runs, nothing is armed and no `hide_at` exists; otherwise it wakes every 200 ms.
- Each wake: compute snapshots; fire due `TimerAtZero`, `PreServiceEnd`, `AutoNext`, `AutoHideMessage` through `command_internal`; publish `Update::Timers` when any snapshot's displayed second, state or the auto readout changed — at most every 250 ms, at least once a second while anything runs (FEAT-3's contract).
- Ticking never bumps `Live.version` and never triggers room prepare/commit.
- Interim stage (until FEAT-3): `ui::stage` shows the first `on_stage` timer that isn't Stopped on today's timer line, with its name, red when overrun; the media countdown keeps priority while video/audio plays.

### 5.9 Editing actions (do_action)

`automation/timer/save|delete`, `automation/message/save|delete`, `automation/macro/save|delete`, `automation/macro/move {id,to}`, `automation/preservice/save`, `service/starts-at {id, at: "HH:MM"|null}`; timer controls `timer/start|pause|reset|restart {id}`, `timer/set-now {id, secs|at}`; `auto/set {on}`. Timer controls are runtime only (no layer change) and idempotent. Validation: name 1–40 characters; durations 1 s–23:59:59; auto-advance 2–600 s; ≤ 30 steps per macro; ≤ 50 timers, 100 templates, 100 macros; at-zero macro must be overlay-only. Deleting something referenced asks "“Sermon” is used by 2 macros. Delete it anyway? Those steps will stop working."; deleting a shown timer or message hides it first (live command).

### 5.10 Restart rule (for the native §8.3 plan)

When crash recovery is built: timers restore from wall-clock instants (a countdown that crossed zero while down doesn't fire its at-zero; notice "“Sermon” reached zero while RustyPresenter was restarting."); `AutoRun` is re-armed from the restored cursor with a full interval and `waiting_for` dropped (versions restart); `hide_at` restores by `shown` id (uuids, not versions) and is dropped if the frame has no such message; `pre` is dropped, so a pre-service end after a restart only hides the countdown.

### 5.11 Phone remote

`server.rs::remote_command` allowlist grows by exactly:
- `RunMacro{macro_id}` — only if `remote: true`; base always required from the phone.
- `ShowMessage{template, …}` — only if the template has `remote: true`.
- `HideMessage`.

No timer control and no auto-advance toggle. `InternalCommand` cannot be expressed in JSON at all. `remote_view` adds `macros: [{id,name,color}]`, `messages: [{id,name,fields}]`, `message_shown` (from the frame). The phone stream forwards `Update::Timers` as `{"type":"timers","timers":[{name,text,over}]}` filtered to `on_remote`.

## 6. Rendering

FEAT-1 owns the overlay layer, its order (§3.8) and `render::overlay` (`render_overlay(fonts, content, text, place, theme_font, canvas, picture)`). FEAT-2 adds **two arms inside that function**, no new rasteriser files:
- **Timer, corner:** at 1080p: caption 30 px weight 600 above time 72 px weight 700; plate `#000000` at 65 %, radius 16, padding 16×24; margin 48 px. Digits use fixed cells as wide as the widest digit so `1:11` → `1:10` doesn't shift.
- **Timer, centre large:** time 220 px weight 700, caption 56 px, no plate, theme shadow.
- **Message banner:** full-width bar (FEAT-1 `BoxKind::Bar`) at 75 %, 12 px from the bottom or top, text 52 px weight 600 centred, up to 3 lines, shrinking to no less than 44 px (≥ 4 % of screen height at 1080p).
- **Theme:** FEAT-2 overlays use the **service theme, or the church default** when the service has none — never the item theme, so they don't jump between items. A deleted theme falls back as FEAT-1 says. Font and text colour come from that theme unless the `OverlayLook` sets them.
- **Bound text:** FEAT-1's `BoundCtx` gains `timers: Arc<TimersView>`; `overlay::bound_text` for `Timer` returns `timers::text(value_ms)` (caption separately). **Every output gets the text the same way**: the room, the operator preview and FEAT-7's stream renderer keep the newest `Update::Timers` and re-render only timer overlays whose text changed (cached by text, no fade, no commit). Prepare keys exclude the time text.
- Message replacement crossfades 300 ms; show/hide fades 400 ms (FEAT-1). Thumbnails never show overlays.
- Cost: one ≤ 900×300 px raster per visible timer per second; nothing while none is shown.

## 7. UI

Extends Plan 3. `@tr("…")` for every visible string; colours from `Theme`.

**New `.slint` files**
- `ui/operator/automation-strip.slint` — `AutomationStrip { in property <[TimerChip]> timers; in property <int> timers-more; in property <[MacroBtn]> macros; in property <bool> macros-more; in property <string> message-label; in property <bool> message-on; in property <string> auto-label; in property <bool> auto-shown; in property <bool> auto-paused; callbacks timer-toggle(string), timer-menu(string), timers-more(), message(), macro(string), macros-more(), auto-toggle() }`. Fixed widths per §3.1. Ids `auto-strip`, `message-btn`, `auto-chip`, `timer-<n>`, `timers-more`, `macro-<n>`, `macros-more`.
- `ui/dialogs/automation.slint` — wide `DialogShell`, Segmented tabs; children `automation-timers.slint`, `automation-messages.slint`, `automation-macros.slint`, `automation-preservice.slint`.
- `ui/dialogs/show-message.slint` — built on the `ask` overlay pattern so it stacks over any open dialog: template ComboBox, generated fields, preview, practice warning, buttons **Hide** (when one is up), **Cancel**, **Show** (primary). Enter = Show, Esc closes only it.
- `ui/widgets.slint`: `TimeField`, `ColorSwatches`. `ui/icons.slint`: `clock`, `bolt`, `message`, `lock`.

**Modified Plan 3 files**
- `topbar.slint` (Automation button `automation-btn`); `live.slint` (hosts the strip under FEAT-1's overlays strip); `slides.slint` (clock badge); `item-edit.slint` (`advance-on`, `advance-secs`, `advance-loop`); `services.slint` (Starts at).
- `src/ui/operator/keys.rs` — **Ctrl+Shift+M** (Cmd+Shift+M on macOS; plain Cmd+M stays the system Minimize) → `KeyAction::ShowMessage`. It is in the `capture-key-pressed` set with F1–F6, so it works while typing and with a dialog or ask overlay open. Help gets the row.
- `src/ui/dialogs/mod.rs` — `DialogKind::Automation(AutomationTab)`; Show message is an ask-layer overlay (`asks += 1`), not a `DialogKind`, so the dialog under it survives. The Automation dialog is not rebuilt by `dialog_refresh` (typed text survives) except its read-only readouts, which follow `Update::Timers`.
- Macro buttons: a stale outcome shows "The screen changed — press the button again." (`Ops::outcome_toast` gets a `from_button` flag; keys stay silent).

**New Rust glue:** `src/ui/operator/automation.rs` (pure `timer_chips`, `macro_buttons` (6-slot rule), `auto_label`, unit-tested); `src/ui/dialogs/automation/{mod,timers,messages,macros,preservice}.rs`; `src/ui/dialogs/show_message.rs`. The operator follower forwards the newest `Update::Timers` to `apply_timers`. `src/ui/room.rs`, `src/ui/preview.rs`, `src/ui/stage.rs` follow `Update::Timers`. `web/remote.html`, `web/remote.js`: §3.7.

**Strings:** core messages in `src/automation/strings.rs` (`pub const`, `fn` for formatted ones); UI via `@tr`; phone strings in one object at the top of `remote.js`.

## 8. Errors

| Situation | Operator sees | State |
|---|---|---|
| Macro Live/Runtime step fails | Notice "“Walk-in”: step 2 — No item called “Announcements” in this service." | Nothing changed |
| Macro Action step fails | Toast "Ran “Walk-in”, but step 6 didn't work: That layout is gone." | Screen part done |
| Macro step from a newer version | "“Walk-in”: step 4 — This step needs a newer RustyPresenter." | Nothing changed |
| Relative macro with stale base | "The screen changed — press the button again." | Nothing changed |
| Message field empty / too long | "Fill in “Number”." / "That message is too long for the screen (200 letters at most)." | Overlay stays open |
| No start time | "Set a start time for this service first (Services → Starts at)." | Timer Stopped |
| Start time passed | "The service start time (10:30) has passed." / "That time (10:30) has passed." | Unchanged |
| Pause a time-of-day countdown | "A countdown to a time can't be paused." | Unchanged |
| Too many timers on screen | "Up to 4 timers can be on the screen at once." | Unchanged |
| At-zero macro not overlay-only | "“Sermon” reached zero, but “X” changes slides, so it didn't run." | Timer Finished |
| Pre-service end skipped | Notice "Countdown ended — nothing else changed because the service has started." | Countdown hidden |
| Overlay preparation fails in the room | v1 §5.3 notice "Could not show that: …"; frame reverts; Message button follows the frame | Committed output |
| Phone runs an unmarked macro/message | "Not allowed from the phone." | Nothing |
| Invalid save | Validation message (§5.9) | Not saved |

## 9. Testing

**Unit tests (core, `FakeClock`, fixed zones via dev-dependency `chrono-tz`)**
- `timers`: `countdown_rounds_up_and_stops_at_zero`, `countdown_overrun_counts_up_with_plus`, `count_up_formats_hours`, `pause_and_resume_carry_elapsed`, `reset_clears_zero_fired_and_set_for_now`, `set_for_now_does_not_change_definition`, `until_time_target_fixed_at_start`, `until_time_overrun_past_60_min_keeps_counting_up`, `start_after_target_runs_at_zero_or_refuses` (service start passed → refused; fixed time 30 min past → refused; 3 h past → tomorrow), `editing_starts_at_does_not_move_running_timer`, `until_time_across_dst_change` (America/Chicago, March and November), `service_start_without_time_errors`, `until_time_refuses_pause`, `at_zero_fires_once`, `snapshot_states_match_feat3_names`, `timer_text_shared_formatter`.
- `lenient`/`model`: `unknown_macro_step_loads_and_round_trips`, `unknown_timer_kind_loads_and_cannot_start`, `unknown_unit_enum_value_loads_as_default`, `bad_time_of_day_clamps_or_drops_field`, `preservice_missing_fields_default`, `old_data_json_loads_with_automation_defaults`, `seed_uses_fixed_ids_and_saves_once`, `seed_not_rerun_after_delete`, `time_of_day_serializes_hh_mm`.
- `messages`: `fields_in_order_without_duplicates`, `double_braces_are_literal`, `stray_brace_is_literal_with_warning`, `fill_requires_every_field`, `fill_trims_and_flattens_newlines`, `fill_rejects_over_200_chars`.
- `live` (refactor): `batch_records_one_history_snapshot`, `batch_failure_restores_next_inst_and_notice`, `operator_next_right_after_auto_next_is_not_lost`, `next_after_clear_all_still_stale_despite_auto_next`, plus the unchanged `delayed_next_cannot_undo_clear_all`, `two_nexts_from_same_state_advance_once`.
- `macros`: `macro_is_one_version_bump`, `failed_step_rolls_back_everything` (incl. a timer started in step 1), `action_steps_run_after_commit_and_report_failure`, `same_id_runs_once_new_id_runs_again`, `phone_macro_after_clear_all_is_stale`, `relative_macro_without_base_rejected`, `overlay_only_macro_needs_no_base`, `item_target_by_number_and_title_case_insensitive`, `show_item_step_slide_one_is_first_slide` (editor 1 → stored 0 → first slide), `missing_reference_names_the_step`, `jump_and_refresh_are_not_steps`.
- `clear matrix`: `clear_all_removes_timer_and_message_overlays`, `f6_removes_timer_and_message`, `logo_keeps_timer_and_message`, `clear_slide_keeps_overlays`, `incoming_item_keeps_overlays`, `show_message_replaces_previous`, `auto_hide_ignores_replaced_message`, `clear_all_clears_message_shown_flag`, `failed_message_prepare_resets_message_button`, `fifth_timer_in_room_refused`.
- `advance`: `auto_next_turns_slides_on_interval`, `loops_to_first_slide`, `stops_on_last_without_loop`, `never_crosses_into_next_item`, `manual_next_restarts_interval`, `clear_slide_stops_until_next_show`, `commit_restarts_due_time`, `auto_next_ignored_after_operator_moved`, `failed_revert_rearms_from_reverted_cursor`, `message_show_does_not_restart_interval`, `one_slide_item_never_arms`, `video_item_ignores_advance`, `practice_auto_advance_uses_active_engine`.
- `preservice`: `build_puts_music_before_loop`, `preservice_loop_keeps_running_after_music_starts`, `next_during_preservice_turns_loop_slide`, `preservice_end_first_item_when_untouched`, `preservice_end_does_nothing_after_operator_took_an_item`, `preservice_end_after_restart_only_hides`, `start_countdown_after_start_time_refused`, `titles_survive_copy_last_week`.
- `at_zero`: `at_zero_rejects_relative_macro_at_save`, `at_zero_skips_macro_made_relative_later`.
- `ticker`: `no_timers_update_without_text_change`, `sleeps_when_idle`, `updates_at_least_once_a_second_while_running`.
- `server`: `remote_macro_allowed_only_when_marked`, `remote_message_allowed_only_when_marked`, `internal_commands_do_not_deserialize`, `remote_timers_filtered_to_on_remote`.
- `render` (golden): `timer_corner_bottom_right`, `timer_center_large_with_caption`, `message_banner_three_lines_min_44px`, `digits_do_not_shift`, `overlay_uses_service_theme_not_item_theme`.
- Layer order: `logo_screen_still_draws_message_banner` — a Slint-testing-backend test on `ui/room.slint` (logo on + message overlay → the overlay item is visible and later in the tree than `logo`) and a FEAT-7 `render::compose` golden image with logo + banner.

**UI tests (Slint testing backend, `Ops::inline`)**
`strip_shows_running_timer_and_toggle_pauses`, `timer_chip_red_when_over`, `strip_overflow_goes_to_more_menu` (8 macros → 5 buttons + More…; 3 timers → 1 chip + "+2 timers"), `ctrl_shift_m_opens_show_message_on_last_template`, `message_shortcut_works_over_open_item_edit` (typed text in Item edit survives), `enter_in_show_message_shows_banner_and_closes`, `empty_field_keeps_overlay_open`, `message_button_follows_frame`, `macro_button_runs_and_toasts`, `macro_stale_toasts_press_again`, `macro_failure_shows_notice`, `auto_chip_shows_countdown_and_pauses`, `item_edit_saves_auto_advance`, `services_starts_at_round_trips`, `preservice_save_creates_locked_macro_first_in_strip`, `set_for_now_prompt_changes_run_only`, `automation_dialog_keeps_typed_text_across_data_update`, `f1_in_automation_dialog_clears_overlays`.

**Manual checks (Windows, macOS, Linux)** — reference laptop: the oldest test machine in `PROJECT_PLAN.md` (dual-core, integrated graphics, 1080p projector or monitor).
- Corner timer over a playing 1080p video: average CPU over 60 s in Task Manager / Activity Monitor / `top` with and without the timer shown; the rise is ≤ 3 percentage points and mpv reports no extra dropped frames over 10 minutes.
- Banner text height ≥ 44 px (4 % of 1080) measured on a screenshot of the room window; a 200-character message fits in 3 lines.
- Lid closed 2 minutes during a running Sermon timer → on wake the readout is within 1 s of a phone stopwatch.
- Change the time zone while a fixed-time countdown runs → readout correct within 1 s.
- Cmd+Shift+M on macOS opens Show message and does not minimize.
- Phone: run a phone macro, show/hide a nursery message, watch the timer line; press F1 then tap a stale phone macro → nothing comes back.

## 10. Dependencies on other FEAT specs

- **FEAT-1 (required first):** `OverlayContent::{Timer, Message}` arms; `Frame.overlays`, overlay put/remove mutators usable from `Live::mutate`; F6 Clear overlays; `BoundCtx` gains `timers`; `render_overlay` gains the two arms; the layer order background < media < slide < logo < overlays (FEAT-1 §6.1). FEAT-1's "core tick `Update::Tick`" is this spec's `Update::Timers`.
- **Plan 3:** files in §7; `Ops`, `DialogShell`, `ask` overlays, key capture set, toasts.
- **FEAT-3:** gets `src/timers.rs`, `TimerSnapshot` with states Stopped/Running/Paused/Finished, `App::timers()`, `Update::Timers`, `timers::text`; macro steps `stage/message/show`, `stage/message/hide`, `stage/layout/use` (Action class).
- **FEAT-4:** `Jump` and `Refresh` are never macro steps; Jump uses the AutoNext-tolerant base rule; a Jump changes `visual_version`, so it restarts the auto-advance interval.
- **FEAT-6:** practice uses FEAT-6's two-engine model: auto-advance and reconcile use `App::with_active_engine`; macros run on the active engine through `App::command`, so clear/logo steps also reach the room exactly as FEAT-6 routes clears; messages and timer overlays go to the practice engine. Lock order data → live → practice → automation. Copy last week/templates copy `starts_at` and `advance`. Practice start/stop are not macro steps.
- **FEAT-7:** **FEAT-7's `compose` must draw logo before overlays** (its current text draws overlays then logo; this is a correction FEAT-7 must take). Timers and messages appear on any screen whose view shows overlays (`LayerMask.props`); no per-source mask. The stream renderer follows `Update::Timers` for timer text. Macro step ScreenView sends `SetScreenView` (Live class, implemented as a FEAT-7 Live mutator).
- **Bible/translations:** none.

## 11. Build order (for the plan)

1. `lenient.rs`, `timers.rs` model + serde + seeding with fixed ids + `TimeOfDay` + `Clock`.
2. Timer state machine, snapshots, `timers::text`, `timer/*` actions.
3. Messages fill/parse.
4. `live.rs` refactor (mutate/commit_change, Origin, visual/manual versions, base freshness) + `apply_batch`; existing tests green.
5. New commands, InternalCommand, clear-matrix rows (needs FEAT-1 overlays).
6. Macros, item targets, Action steps, relative-macro base.
7. Auto-advance reconcile + commit hook; pre-service build and guarded end; at-zero rules.
8. Ticker + `Update::Timers`.
9. Render arms (golden) + room/preview wiring.
10. Strip, Ctrl+Shift+M, Show message overlay, item edit, services start time.
11. Automation dialog (four tabs).
12. Phone remote + allowlist; stage interim line; manual checks.

## 12. Decisions for Jake to review

Each line: **default chosen** — main alternative.

1. **Built-in timers** Service start, Sermon (30 min, keeps counting), Stopwatch — start with no timers.
2. **Built-in messages** Nursery, Vehicle, Custom — only Custom.
3. **One message at a time**, newer replaces older — queue/rotate several.
4. **Static banner** along the bottom — scrolling ticker (costs smooth redraw on old laptops).
5. **Messages stay up until hidden** (optional per-template "Hide after") — auto-hide after 30 s by default.
6. **Timers and messages follow the overlay rules: Clear All and Clear overlays (F6) remove them; Logo and Clear Slide/Media/Audio keep them** — Logo also removes timers, or messages survive Clear All.
7. **Timer room position bottom-right**, small plate — centre large by default.
8. **Sermon shows over-time as `+m:ss` (red)**; Service start stops at 0:00 — always stop at zero.
9. **A countdown's target time is fixed when it starts; starting after the time has passed is refused** ("has passed"); a fixed time more than an hour past counts to tomorrow — start anyway and run "at zero" at once.
10. **Editing the service start time doesn't move a running countdown**; restarting picks it up — running countdowns follow edits.
11. **Service start time is per service** (copied by "copy last week") — one church-wide time in Settings.
12. **Pre-service end default: just hide the countdown.** Logo / Clear / First item happen only if nobody changed the screen since Start countdown — always do the end action.
13. **Pre-service picks loop and music by item title** — by position.
14. **Pre-service starts the music before the loop; navigation is unchanged, so taking an audio item live by hand still ends a running loop** — change `go()` so audio items never move the cursor.
15. **Auto-advance default 8 s, loops by default** — 10 s, or stop at the end.
16. **Auto-advance never moves to the next service item** — option to continue.
17. **One interval per item** — per-slide durations.
18. **Clears and Logo stop auto-advance until the operator shows a slide of that item again** — resume automatically.
19. **One-slide items never auto-advance; stage/phone "next" on a loop's last slide shows the next service item** (what Next would do) — show "slide 1" as next while looping.
20. **A Next pressed just after an automatic slide turn still counts** (moves on from the new slide) — reject it and toast "The slide moved by itself — press again".
21. **Macros have no waits; the screen part is all-or-nothing, stage steps follow** — waits with sequential, non-atomic running.
22. **"At zero" may only run macros that show or hide things** (timers, messages, overlays, stage message) — any macro.
23. **No macro shortcuts; the message shortcut is Ctrl/Cmd+Shift+M and works over dialogs** — Shift+F-keys for macros, or F7 for messages.
24. **Strip shows up to 6 macro slots (5 + More… when more) and 2 timer chips** — a separate macro panel/tab.
25. **Phone may run only macros and messages marked for it (default off); slide-changing macros need the phone's view to be current**; no direct timer control — direct timer start/pause on the phone.
26. **In practice mode messages and room timers show only in the practice preview** (with a warning in Show message) — urgent messages reach the real room even during practice.
27. **The livestream shows timers and messages whenever its screen view shows overlays** — a per-source mask in FEAT-7 (e.g. stream hides messages).
28. **Chip "Set for now…" changes this run only** — it saves the new length permanently.
29. **Stage interim line**: the first running "Show on stage" timer replaces the clock line unless media plays — wait for FEAT-3.
30. **Timers keep running when hidden or cleared**; only Reset stops them — Clear All also resets timers.
31. **Deleting a built-in is permanent** — a "restore built-ins" button.
32. **12/24-hour follows the computer's locale** — always 12-hour.
33. **Timer and message overlays use the service theme (or church default), never the item theme** — follow the live item's theme.
34. **Room timers and messages don't count toward FEAT-1's 8-overlay limit; at most 4 timers on screen** — share one limit.
35. **"Last used message" is remembered only until the app closes** — saved in data.json.

## 13. Review notes

- P1 "Audio go()": fixed by ordering music before the loop, not by changing `go()`; changing where Next goes after an audio item is a navigation change for every service (v1 §7.2) and is listed as decision 14 instead.
- P1 "layer order": FEAT-1 §6.1 already says overlays above logo; the contradiction is in FEAT-7's `compose`, which this spec cannot edit — §10 records the correction FEAT-7 must make.
- P3 "loop next on stage": kept the current behaviour (decision 19) rather than changing FEAT-3's next-slide rule.
- P3 "crash recovery": moved out of scope; §5.10 gives the restore rule for the native §8.3 plan.
- P3 "time library": chose `chrono` (already used by FEAT-1/3/6) over `jiff` to keep one time model without rewriting three specs; DST cases are covered by generic `TimeZone` code and `chrono-tz` tests.
