# Song auto-follow (lyrics follow the singers) — design

Date: November 9, 2026. Owner: Jake. Status: draft for Jake's review. Comes **before** livestreaming (1.3); planned as **1.3** with livestreaming moving to 1.4.

## 1. What Jake wants

- For songs, the app listens and **moves the lyric slides by itself** as the band sings. "Automatic would be really really cool"; Manual and other modes must stay.
- **Early but not too early:** "about a second before people sing it" — the computer hears late, so it must not wait to hear the next line.
- **No words between parts** where nobody sings (instrumentals), so people don't anticipate.
- **Singers improvise:** talk in the middle of a song, pray, quote a verse, repeat a line — the screen must not jump around.
- **Sound from the main mix** for now; other inputs as options. "A lot of thought put into this."

Assumptions (correct me): runs fully on the church PC, offline (no cloud, no audio leaves the building); a mid-range Windows PC without a big GPU must manage at least the small model; English first, any language Whisper knows works.

**Success:** on a normal Sunday set, Automatic gets every slide change right with the next slide's words on screen about **2 s (setting 0.5–4 s; Jake: "maybe even 2 to 3 seconds") before the first word of the next slide is sung**; it blanks during instrumentals; it never jumps on talking/prayer; when it is unsure it holds, never guesses; the operator can always take over instantly.

## 2. Modes

- **Manual** — today's behaviour.
- **Suggest** — the app highlights the slide it thinks is next ("ready" glow + the stage/phone shows it); the operator presses Next/Space or clicks.
- **Automatic** — the app moves slides itself. The operator's Next/Back/click always wins at once; Automatic then re-locks onto the singers when it is sure again.
- Per song item (Item edit), per service default, and a big **Auto** toggle in the Live column + a key (A) and a **Hold** key (H, "don't move now"). The phone remote and the stage screen show the state.

## 3. How it works (approach)

Chosen: **our own Rust lyrics follower (forced aligner + beat/voice tracker) on a small CTC acoustic model run in pure Rust (Candle)**; Whisper only as an optional extra (Bible-verse detection, off-script check). Why: we already know the words, so the task is *alignment*, not transcription. A CTC model (wav2vec2-style; multilingual phoneme model, see §9 for the exact models and licences) gives sound probabilities every 20 ms, streams with low latency and doesn't invent words; our aligner walks those probabilities through the known lyrics (Viterbi/beam over the lyric phoneme sequence), which is the standard approach for lyrics-to-audio alignment. Later: fine-tune the acoustic model on sung audio (public lyrics-alignment data + the church's own rehearsal recordings) to improve singing accuracy. Rejected: training a speech model from scratch (needs thousands of hours of labelled singing and large GPU clusters); Whisper as the core (transcribes in chunks → slow to react, can hallucinate words over music, doesn't use the known lyrics well); cloud speech (needs internet, sends church audio away, costs money). No C++ toolchain (Jake: never MSVC); new crates come from Jake's PC once (cargo fetch there, sent to the cloud vendor dir).

Pipeline (one background thread group, never on the UI thread):
1. **Capture** the main mix (cpal input device chosen in settings; later the leader's channel or any input), 16 kHz mono.
2. **Clean-up:** band-pass for voice + optional vocal isolation (later: a small source-separation model if the PC is fast enough).
3. **Voice activity (VAD):** singing / speaking / instrumental / silence, per 100 ms, with a short history. Distinguishes singing from talking (pitch stability, harmonic ratio, tempo-locked rhythm) — used for blanking and to ignore talk.
4. **Acoustic model:** a CTC model (Candle) turns each 20 ms frame of audio into letter/phoneme probabilities, streaming (rolling ~1 s context, ~0.2–0.4 s latency). Model size picked from a speed check (Fast / Balanced / Best).
5. **Aligner (ours):** the song's slides (planned arrangement order, but every section a candidate) become a phoneme/letter sequence (a small Rust grapheme-to-phoneme step + per-song pronunciation fixes). A streaming beam search over positions in that sequence scores the incoming probabilities; it prefers continuing forward in the plan, allows jumps to other sections only with strong evidence (a run of ≥4 well-matched words), never jumps on one word, and reports a confidence. When the match score falls (talk, prayer, ad-libs), it reports "off-script" instead of guessing. Optional Whisper pass (if the PC is fast) double-checks off-script audio and spots quoted Bible references.
6. **Prediction, not reaction (the key point).** Hearing words always lags (the aligner ~0.3–0.5 s, Whisper 1–2 s), so word matching is only used to *confirm and correct* where the singers are. The switch itself is timed by a **low-latency beat/onset and voice tracker (~50 ms)** plus the known lyrics: from the current position, the remaining syllables on the slide, and the measured singing pace (syllables per beat, beats per second), it predicts when the next line starts and switches **lead time** (default 2 s) before it. Rehearsal learning stores each line's duration for this team, making the prediction near-exact. If the prediction and recognition disagree, recognition wins at the next safe moment (never mid-line flicker).
7. **Decision:** moves the live slide (Automatic) or highlights it (Suggest) through the normal live engine (same path as Next, so undo/stage/phone/livestream all follow).

## 4. Rules (the careful part)

- **Timing (default):** the next slide appears **2 s before its first word is sung** (predicted, §3.6); setting **lead time 0.5–4 s**; never before the last line of the current slide has started (so nobody reads too far ahead).
- **Instrumentals:** if no singing for N seconds (default 3, setting, or off), **blank the lyrics** (keep background); bring words back as singing starts — or just before if the beat shows they are about to start. Never show the next section's words during a long instrumental.
- **Talking / prayer / ad-libs:** words that don't match the song hold the current slide; long talking blanks the lyrics after N seconds (setting); returns when singing of song words resumes.
- **Repeats and tags:** repeating a line or the last line keeps the current slide; a repeated chorus outside the plan is followed only on strong evidence.
- **Out-of-plan jumps:** "Plan first, jump when sure" — needs a run of matched words (default 4) inside another section.
- **Bible verse quoted mid-song:** offers "Show John 3:16?" as a one-tap suggestion; never shows it by itself.
- **End of song:** after the last slide, holds; then blanks after the outro; never jumps to the next service item by itself (setting may allow it).
- **Confidence:** every decision has a confidence; below the threshold it holds (Automatic) or just highlights (Suggest).
- **Operator wins:** any manual move pauses Automatic for that song until it is sure again (or until the next item, setting).

## 5. Interface

- Live column: **Auto** chip (Manual / Suggest / Auto), a small listening light (off / listening / sure / unsure / holding / blank) and the line it thinks is being sung; Hold button.
- Item edit: per-song mode; "Song language".
- Settings → Auto-follow: input device + level meter + **Test** ("say/sing a line"); model size (Fast / Balanced / Best) with download; lead time slider (0.5–4 s, default 2 s); blank after N s of no singing; blank after N s of talking; jump strictness; suggest Bible verses on/off.
- Stage screen: "Auto: following — Chorus 2"; phone remote: same chip + Hold.
- **Review log** per service: each change with time, reason ("heard 'grace that saved'"), confidence; **Practice**: run Auto-follow on a recording (e.g. last Sunday's recording from 1.0 record-to-disk) to tune settings at home.
- **Rehearsal learning:** run it during rehearsal; it remembers this team's arrangement order and tempo per song for Sunday.

## 6. Errors and limits

- No input / silent input: warning; Automatic acts as Manual.
- PC too slow (recognition falls behind real time): drops to a smaller model, then to Suggest, with a plain message.
- Model not downloaded: offers the download (sizes shown); works offline afterwards.
- Unknown language or lyrics in a different language than sung: falls back to Suggest.
- Never moves while the item isn't a song, while a video plays, or when the operator has Hold on.

## 7. Testing

- Aligner, beat/pace predictor (target: switch within ±0.3 s of the lead time on test recordings), blanking rules and decisions unit-tested on scripted word streams with timestamps (no audio): forward progress, repeats, out-of-plan chorus, talk, prayer, instrumentals, tags, endings.
- VAD tested on short labelled audio clips (sung / spoken / instrumental) kept in the repo (royalty-free or self-made).
- End-to-end on a few recorded public-domain hymns (e.g. "Amazing Grace" sung + spoken interlude) measuring: correct changes, early/late margin, false jumps — must hit 0 false jumps on the test set.
- Speed test of each model size on the CI box; on Jake's PC: Practice mode on last Sunday's recording before any live use.

## 8. Build order

1. **Aligner + predictor + rules + Suggest/Auto plumbing** with a fake recognizer and fake beat tracker (testable now).
2. **Audio capture + low-latency beat/voice tracker + VAD + blanking.**
3. **CTC acoustic model in Candle + model download + speed check;** optional Whisper extra.
4. **UI:** chips, light, settings, test, stage/phone, log, Practice mode.
5. **Rehearsal learning, Bible-verse suggestion, vocal isolation (if fast enough).**

## 9. Open points

- New Rust crates (candle-core, candle-nn, candle-transformers, cpal, rubato; tokenizers only for the Whisper extra) must be fetched on Jake's PC once and copied to the cloud build — one script for Jake to run.
- **Models (Jake: downloaded, not cloud, legal, totally free):** only licences that allow any use (Apache-2.0 / MIT), shipped with their licence text; downloaded once from Hugging Face, pinned by checksum, then fully offline.
  - Main ear: `facebook/wav2vec2-xlsr-53-espeak-cv-ft` — **Apache-2.0**, multilingual phoneme CTC (53 pre-training languages), the right shape for lyrics alignment in any language. ~1.2 GB in fp32; shipped as fp16/int8 (~300–600 MB).
  - Fast ear for slower PCs: `facebook/wav2vec2-base-960h` — **Apache-2.0**, English letters CTC (~360 MB, smaller quantised).
  - Optional extra: OpenAI Whisper (tiny/base/small) — **MIT**.
  - **Not allowed:** Meta MMS models (`mms-300m`, `mms-1b-all`): CC-BY-NC 4.0 (non-commercial).
- Later: fine-tuning the acoustic model on singing (public lyrics-alignment data + rehearsal recordings, with permission).
