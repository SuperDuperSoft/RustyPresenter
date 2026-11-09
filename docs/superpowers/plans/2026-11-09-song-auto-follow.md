# Song Auto-Follow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Song lyric slides follow the singers by themselves (Manual / Suggest / Automatic), showing the next words ~2 s before they are sung, blanking between sung parts, never jumping on talk or a guess.

**Architecture:** A new `src/follow/` module: pure-Rust decision core (lyrics model → aligner → predictor → rules → `FollowAction`) fed by an audio front end (FFmpeg child giving 16 kHz mono PCM → voice/onset tracker) and an acoustic model (CTC phoneme probabilities; Candle, part B). Actions go through `App::command` (`Command::Show`, `ClearSlide`), so undo, stage, phone and livestream follow as for a click.

**Tech Stack:** Rust, Slint 1.18 (`crates/rp-ui`), FFmpeg (already fetched by the app), Candle (part B, new crates fetched on Jake's PC).

**Spec:** `docs/superpowers/specs/2026-11-09-song-auto-follow-design.md`

## Global Constraints

- Offline only; no audio leaves the PC. Models: Apache-2.0 / MIT only (`facebook/wav2vec2-xlsr-53-espeak-cv-ft`, `facebook/wav2vec2-base-960h`, optional Whisper). MMS models are forbidden (CC-BY-NC).
- Lead time default **2 s**, setting **0.5–4 s**; never before the last line of the current slide has started.
- Out-of-plan jump needs **≥4 consecutive well-matched words**; never jump on one word; below confidence threshold → hold (Automatic) / highlight only (Suggest).
- Blank lyrics after **3 s** of no singing (setting, or off); never show the next section during an instrumental.
- Operator Next/Prev/click/Hold always wins immediately; Automatic re-locks only when sure.
- Never acts when the live item isn't a song, while a video plays, or when Hold is on.
- No UI-thread work: audio, model and aligner run on background threads.
- No MSVC; new crates only as vendored on Jake's PC (part B). All user text via `tr!`/`@tr`; rules in /root/AGENT_RULES.md.

## Review Focus

1. **A song whose sections repeat word-for-word (chorus ×3):** must follow the planned order and pick the *next* chorus occurrence, not jump back to the first. → Task 2 test `repeated_chorus_follows_plan_order`.
2. **Operator moves the slide by hand mid-line:** Auto must stop fighting for the rest of the line and re-lock from the new position. → Task 4 test `manual_move_wins_and_relocks`.
3. **Song with no sections / one big slide / empty lyrics or only "{Instrumental}" tags:** must not panic; Auto stays idle. → Task 1 test `odd_songs_never_panic`.
4. **Audio device disappears or FFmpeg exits mid-song:** Auto falls back to Manual with a notice, no hang. → Task 5 test `input_lost_falls_back_to_manual`.
5. **Item changes (operator goes to the sermon) while Auto holds state from the song:** no stray slide moves on the new item. → Task 4 test `leaving_the_song_stops_auto`.

---

## Part A — decision core + audio front end (no new crates)

### Task 1: Lyrics model

**Files:** Create `src/follow/mod.rs`, `src/follow/lyrics.rs`, `src/follow/lyrics_tests.rs`; Modify `src/lib.rs` (add `pub mod follow;`).

**Interfaces:**
- Produces: `pub struct SongText { pub words: Vec<Word>, pub slides: Vec<SlideSpan> }`; `pub struct Word { pub text: String, pub key: String /* sound-alike key */, pub syllables: u8, pub slide: usize, pub line: usize }`; `pub struct SlideSpan { pub slide: usize, pub first_word: usize, pub last_line_first_word: usize, pub end_word: usize, pub section: Option<String>, pub instrumental: bool }`; `pub fn SongText::from_slides(slides: &[crate::model::Slide]) -> SongText`; `pub fn sound_key(word: &str) -> String`; `pub fn syllables(word: &str) -> u8`.

- [ ] Step 1: tests `words_carry_slide_and_line`, `sound_key_matches_alikes` (`sound_key("grace") == sound_key("grays")`, `sound_key("night") == sound_key("knight")`), `syllables_counts` (`amazing`→3, `grace`→1, `hallelujah`→4), `instrumental_tag_slides_have_no_words`, `odd_songs_never_panic` (empty slides, one slide, only tags, punctuation/emoji, non-Latin text).
- [ ] Step 2: run `RP_TIMEOUT=1800 /root/bin/rpcargo test --offline --lib -- follow::lyrics` → fail.
- [ ] Step 3: implement. `sound_key`: lowercase, strip punctuation/diacritics, Double-Metaphone-style simplification (write a small one; no crate). Words of non-Latin scripts keep their lowercase form as key.
- [ ] Step 4: tests pass. Step 5: commit.

### Task 2: Streaming aligner

**Files:** Create `src/follow/align.rs`, `src/follow/align_tests.rs`.

**Interfaces:**
- Consumes: `SongText`, `Word`.
- Produces: `pub struct Heard { pub key: String, pub at: f64 /* seconds */, pub conf: f32 }`; `pub struct Aligner`; `Aligner::new(song: &SongText, plan: &[usize] /* planned slide order */) -> Aligner`; `fn push(&mut self, heard: &Heard) -> Position`; `fn reset_to(&mut self, slide: usize)`; `pub struct Position { pub word: Option<usize>, pub plan_index: usize, pub confidence: f32, pub off_script: bool }`.

- [ ] Step 1: tests (word streams with timestamps, no audio): `follows_forward_in_plan`, `one_matching_word_never_jumps`, `out_of_plan_jump_needs_four_words` (3 words → stays; 4 → jumps), `repeated_chorus_follows_plan_order`, `off_script_talk_reports_off_script` (10 unrelated words → `off_script == true`, position unchanged), `misheard_words_tolerated` (1 in 4 wrong keys still advances), `repeat_last_line_stays`.
- [ ] Step 2: run → fail.
- [ ] Step 3: implement a beam search over (plan_index, word) states: forward-in-plan transitions cheap, skips within a slide cheap, other-section jumps cost a penalty that only ≥4 consecutive matches overcome; match score from key equality / edit distance ≤1. Confidence = margin between the best and second-best beam. Beam width 64.
- [ ] Step 4: pass. Step 5: commit.

### Task 3: Pace predictor and blanking/hold rules

**Files:** Create `src/follow/rules.rs`, `src/follow/rules_tests.rs`.

**Interfaces:**
- Consumes: `Position`, `SongText`.
- Produces: `pub struct Settings { pub mode: Mode, pub lead_s: f32 /* 0.5..=4.0, default 2.0 */, pub blank_after_s: Option<f32> /* default Some(3.0) */, pub talk_blank_after_s: Option<f32> /* default Some(8.0) */, pub jump_words: u8 /* default 4 */, pub min_conf: f32 }`; `pub enum Mode { Manual, Suggest, Auto }`; `pub enum Voice { Singing, Speaking, Music, Silence }`; `pub struct Rules`; `Rules::new(settings, song: &SongText) -> Rules`; `fn tick(&mut self, now: f64, pos: &Position, voice: Voice, onsets_per_s: f32, live_slide: usize) -> Option<FollowAction>`; `pub enum FollowAction { Show(usize), Suggest(usize), Blank, Unblank(usize) }`.

- [ ] Step 1: tests (fake clock): `switches_lead_seconds_before_next_line` (pace 3 syllables/s, 6 syllables left → Show at 2 s − lead), `lead_is_clamped_0_5_to_4`, `never_before_last_line_started`, `blanks_after_three_seconds_of_music`, `instrumental_never_shows_next_words`, `talk_holds_then_blanks`, `unblanks_when_song_words_return`, `suggest_mode_only_suggests`, `low_confidence_holds`, `last_slide_holds_then_blanks_never_next_item`.
- [ ] Step 2: run → fail.
- [ ] Step 3: implement. Pace = exponential moving average of syllables/second from aligned word times; predicted next-line start = now + remaining syllables / pace; act when `predicted - now <= lead_s` and the position is on the last line.
- [ ] Step 4: pass. Step 5: commit.

### Task 4: Live engine hookup (Follower)

**Files:** Create `src/follow/engine.rs`, `src/follow/engine_tests.rs`; Modify `src/app.rs` (own a `Follower`, call `follower.on_live_change(..)` from `App::command` when the live item/slide changes by any other cause), `src/model.rs` (`ServiceItem.follow: Option<Mode>`, `Settings.follow: follow::Settings` with serde defaults).

**Interfaces:**
- Consumes: Tasks 1–3; `App::command(Envelope{cmd: Command::Show{item_id, slide}})`, `Command::ClearSlide`.
- Produces: `pub struct Follower`; `Follower::start(app: Weak<App>) -> Follower` (spawns the worker); `fn feed(&self, ev: FollowEvent)` where `pub enum FollowEvent { Heard(Heard), Voice(Voice, f64), Onsets(f32, f64), InputLost(String) }`; `fn state(&self) -> FollowState` (`pub struct FollowState { mode: Mode, light: Light, line: Option<String>, holding: bool }`, `pub enum Light { Off, Listening, Sure, Unsure, Holding, Blank }`); `fn set_hold(&self, on: bool)`; `fn set_mode(&self, item: Uuid, mode: Mode)`.

- [ ] Step 1: tests with a fake app clock and scripted events: `auto_shows_next_slide_via_command`, `manual_move_wins_and_relocks`, `leaving_the_song_stops_auto`, `hold_stops_moves`, `video_playing_never_moves`, `non_song_item_idle`, `suggest_sets_highlight_not_live`.
- [ ] Step 2: run → fail. Step 3: implement (one worker thread, channel in, actions out through `App::command` with a `source: Follow` marker so manual moves can be told apart). Step 4: pass. Step 5: commit.

### Task 5: Audio front end (FFmpeg capture + voice/onset tracker)

**Files:** Create `src/follow/audio.rs`, `src/follow/voice.rs`, tests alongside; reuse FFmpeg lookup from `src/record/run.rs` (`ffmpeg_path()` or its equivalent — move it to `src/platform/tools.rs` if private).

**Interfaces:**
- Produces: `pub fn input_devices() -> Vec<String>` (FFmpeg `-list_devices` dshow on Windows, pulse/alsa sources on Linux, avfoundation on Mac); `pub struct Capture`; `Capture::start(device: &str, tx: Sender<FollowEvent>) -> Result<Capture, String>` (FFmpeg → `-f s16le -ar 16000 -ac 1 -`, 20 ms frames); `pub struct VoiceTracker`; `fn frame(&mut self, pcm: &[i16], at: f64) -> (Voice, f32 /* onsets per second */)`.

- [ ] Step 1: tests: `voice_tracker_labels_clips` on small generated signals (sine+vibrato = Singing, noise bursts = Music, silence = Silence; speech-like = Speaking via pitch instability), `onset_rate_from_clicks` (clicks at 2/s → ~2.0), `input_lost_falls_back_to_manual` (fake FFmpeg exits → `InputLost` event → Follower state Manual + notice).
- [ ] Step 2: fail. Step 3: implement: energy + zero-crossing + autocorrelation pitch (own small code, no FFT crate needed; write a radix-2 FFT if needed in `voice.rs`). Step 4: pass. Step 5: commit.

### Task 6: UI (Live chip, light, Hold, settings, stage, phone)

**Files:** Modify `ui/operator/live.slint` (+ Rust in `src/ui/operator/live.rs`), new `ui/dialogs/settings-follow.slint` + `src/ui/dialogs/settings_follow.rs`, Item edit (per-song mode), stage view (`src/stage/view.rs` state line), phone remote (`web/remote.js` chip + Hold, `src/i18n/remote_strings.rs`), shortcuts (A = Auto on/off, H = Hold) in the existing key layer.

**Interfaces:** Consumes `Follower::state/set_hold/set_mode`, `follow::audio::input_devices`.

- [ ] Step 1: tests (Slint testing backend): `auto_chip_cycles_modes`, `hold_key_holds`, `light_shows_state`, `settings_lead_slider_clamps`, `stage_shows_follow_line`, `phone_has_auto_and_hold`.
- [ ] Step 2: fail. Step 3: implement. Settings page: input device + level meter + Test; lead time slider (0.5–4 s, default 2 s); blank after N s of no singing; blank after N s of talking; jump strictness (words); model size (part B; greyed "download in a later update" until then). Step 4: pass. Step 5: commit.

### Task 7: Review log + Practice mode

**Files:** Create `src/follow/log.rs`; Practice: run the pipeline on a recording file (FFmpeg file input instead of a device) from Settings → Auto-follow → Practice.

- [ ] Steps: tests `log_records_reason_and_confidence`, `practice_runs_on_a_file` (fake FFmpeg feeding a WAV) → implement → pass → commit.

## Part B — the ear (needs new crates from Jake's PC)

### Task 8: Fetch and vendor crates

- [ ] Write `fetch-follow-crates.cmd` (Jake runs it once): adds `candle-core`, `candle-nn`, `candle-transformers`, `safetensors` to a scratch copy of Cargo.toml, runs `cargo fetch`, tars the registry cache into `.tools\xfer\crates-follow.tar`. Cloud: vendor them, add to Cargo.toml/Cargo.lock under a default-on `follow-model` feature.

### Task 9: CTC acoustic model in Candle

**Files:** Create `src/follow/model.rs` (wav2vec2 feature encoder + transformer + CTC head, loading safetensors), `src/follow/fetch.rs` (download from Hugging Face, pinned SHA-256, licence file kept), `src/follow/g2p.rs` (letters → espeak-style phonemes for the xlsr model; letters for base-960h).

**Interfaces:** Produces `pub trait Ear { fn frames(&mut self, pcm: &[i16]) -> Vec<Heard>; }` with `CtcEar` (real) and `FakeEar` (tests). Aligner keys switch from sound-alike words to phoneme strings when the phoneme model is used (Task 2's `Heard.key` stays a string).

- [ ] Steps: tests `fake_ear_drives_follower_end_to_end`, `model_loads_and_runs_on_silence` (tiny random-weight model built in the test), `fetch_checks_sha256`, `speed_check_picks_model` → implement → pass → commit. Quantise to fp16/int8 on load if Candle supports it on CPU; otherwise ship fp16 safetensors.

### Task 10: End-to-end tuning

- [ ] Recorded public-domain hymn clips (self-made, sung + spoken interlude) in `tests/follow/`; integration test measures changes, lead-time error (target ±0.3 s), false jumps (target 0).
- [ ] Jake: Practice mode on last Sunday's recording, then a rehearsal, before a Sunday.

Later (not in this plan): rehearsal learning, Bible-verse suggestion (Whisper), vocal isolation.
