# Song auto-follow (lyrics follow the singers) — design

Date: November 9, 2026. Owner: Jake. Status: draft for Jake's review. Comes **before** livestreaming (1.3); planned as **1.3** with livestreaming moving to 1.4.

## 1. What Jake wants

- For songs, the app listens and **moves the lyric slides by itself** as the band sings. "Automatic would be really really cool"; Manual and other modes must stay.
- **Early but not too early:** people must see the next words in time, but not read ahead.
- **No words between parts** where nobody sings (instrumentals), so people don't anticipate.
- **Singers improvise:** talk in the middle of a song, pray, quote a verse, repeat a line — the screen must not jump around.
- **Sound from the main mix** for now; other inputs as options. "A lot of thought put into this."

Assumptions (correct me): runs fully on the church PC, offline (no cloud, no audio leaves the building); a mid-range Windows PC without a big GPU must manage at least the small model; English first, any language Whisper knows works.

**Success:** on a normal Sunday set, Automatic gets every slide change right with the next words appearing during the last word or two of the current slide; it blanks during instrumentals; it never jumps on talking/prayer; when it is unsure it holds, never guesses; the operator can always take over instantly.

## 2. Modes

- **Manual** — today's behaviour.
- **Suggest** — the app highlights the slide it thinks is next ("ready" glow + the stage/phone shows it); the operator presses Next/Space or clicks.
- **Automatic** — the app moves slides itself. The operator's Next/Back/click always wins at once; Automatic then re-locks onto the singers when it is sure again.
- Per song item (Item edit), per service default, and a big **Auto** toggle in the Live column + a key (A) and a **Hold** key (H, "don't move now"). The phone remote and the stage screen show the state.

## 3. How it works (approach)

Chosen: **on-device Whisper (pure Rust, Candle) + a lyrics aligner + voice activity**. Rejected: cloud speech (needs internet, sends church audio away, costs money); sound-alike matching only (worse with talk/ad-libs). No C++ toolchain (Jake: never MSVC); new crates come from Jake's PC once (cargo fetch there, sent to the cloud vendor dir).

Pipeline (one background thread group, never on the UI thread):
1. **Capture** the main mix (cpal input device chosen in settings; later the leader's channel or any input), 16 kHz mono.
2. **Clean-up:** band-pass for voice + optional vocal isolation (later: a small source-separation model if the PC is fast enough).
3. **Voice activity (VAD):** singing / speaking / instrumental / silence, per 100 ms, with a short history. Distinguishes singing from talking (pitch stability, harmonic ratio, tempo-locked rhythm) — used for blanking and to ignore talk.
4. **Recognition:** Whisper (tiny/base/small, user picks; auto-picked from a speed check) on a rolling 2–5 s window every ~0.5 s, with the **song's own lyrics as the prompt** (Whisper's initial prompt / vocabulary bias) so it hears the song's words.
5. **Aligner:** the song's slides (in the planned arrangement order, but every section is a candidate) become a word sequence with phonetic keys. Heard words are matched with a fuzzy, sound-alike score (Double Metaphone + edit distance) using a streaming alignment (beam over positions) that prefers continuing forward in the plan, allows jumps to other sections only with strong evidence (several consecutive matched words), and never jumps on one word.
6. **Tempo and position:** tracks the singers' word rate to **predict** when they reach the last words of the current slide, so the switch happens *before* Whisper's 1–2 s lag would allow.
7. **Decision:** moves the live slide (Automatic) or highlights it (Suggest) through the normal live engine (same path as Next, so undo/stage/phone/livestream all follow).

## 4. Rules (the careful part)

- **Timing (default):** switch while the last 1–2 words of the current slide are sung (≈0.5–1 s before the next line), scaled by tempo; slider **Earlier ↔ Later**; never before the last line of the current slide has started.
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
- Settings → Auto-follow: input device + level meter + **Test** ("say/sing a line"); model size (Fast / Balanced / Best) with download; timing slider; blank after N s of no singing; blank after N s of talking; jump strictness; suggest Bible verses on/off.
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

- Aligner, timing predictor, blanking rules and decisions unit-tested on scripted word streams with timestamps (no audio): forward progress, repeats, out-of-plan chorus, talk, prayer, instrumentals, tags, endings.
- VAD tested on short labelled audio clips (sung / spoken / instrumental) kept in the repo (royalty-free or self-made).
- End-to-end on a few recorded public-domain hymns (e.g. "Amazing Grace" sung + spoken interlude) measuring: correct changes, early/late margin, false jumps — must hit 0 false jumps on the test set.
- Speed test of each model size on the CI box; on Jake's PC: Practice mode on last Sunday's recording before any live use.

## 8. Build order

1. **Aligner + rules + Suggest/Auto plumbing** with a fake recognizer (testable now).
2. **Audio capture + VAD + blanking.**
3. **Whisper (Candle) + model download + speed check.**
4. **UI:** chips, light, settings, test, stage/phone, log, Practice mode.
5. **Rehearsal learning, Bible-verse suggestion, vocal isolation (if fast enough).**

## 9. Open points

- New Rust crates (candle-core, candle-nn, candle-transformers, tokenizers, cpal, maybe rubato) must be fetched on Jake's PC once and copied to the cloud build — one script for Jake to run.
- Model files (~75–500 MB) download on first use from Hugging Face; pinned by checksum.
