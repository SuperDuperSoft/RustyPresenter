# Livestreaming (RustyPresenter 1.3) — design

Date: November 9, 2026. Owner: Jake. Status: draft for Jake's review.

## 1. What Jake wants (his words, summarised)

- RustyPresenter streams the service itself: YouTube, Facebook Live, Instagram Live, X, TikTok, Twitch, and **any** other service (custom RTMP/RTMPS/SRT address + key). Our own code, not OBS.
- Viewers see **camera + slides mixed** (camera full, lyrics as a lower third, full slide for presentations, picture-in-picture).
- The **same person** who runs the slides runs the stream, and it is **mostly automatic** (like ProPresenter's stream screen + Looks, but needing no setup).
- **All camera kinds:** USB webcams and capture cards (any video input device), several cameras with switching, network cameras (NDI, RTSP, PTZ), a phone as a camera.
- **Sound** like OBS / ProPresenter: sound board in, app sound mixed in, with simpler choices for small setups.
- **Sign in or stream key:** "whatever works best and easiest"; stream names, titles and descriptions must be **easy to name and rename**.
- **Easy by default, advanced settings for power users.** Full **recording and output settings**. Ideas taken from OBS, Streamlabs, ProPresenter and church streaming services.

Assumptions (not said by Jake; correct me): Windows first (Mac/Linux later, same design); a church PC with a mid-range GPU or iGPU; 5–20 Mbit/s upload; services of 1–2 hours.

**Success:** a volunteer who has never streamed presses one button on Sunday and the service goes out to YouTube + Facebook with camera, lyrics lower thirds and good sound; if the internet blips, it recovers without anyone touching it; the recording is safe on disk either way.

## 2. What others do (research, Nov 2026)

- **ProPresenter 7 "Capture":** pick which screen to capture; resolution/fps (incl. custom 1080×1920 vertical); stream RTMP/RTMPS or Resi; record at the same time; alpha recording (ProRes 4444 / HAP Alpha) for keying; cameras come in as live video in slides/props; separate stream screen with its own **Look**; slide actions/macros switch Looks. Basically one destination at a time.
- **OBS Studio 32:** scenes/sources, Studio Mode (preview/program), simple vs advanced output (encoder, rate control, keyframes, profile), recording (format, remux, file splitting with chapter markers, multi-track audio, replay buffer), new audio mixer, filters (noise suppression, gate, compressor, limiter, gain, EQ), stream delay, auto-reconnect, dynamic bitrate, bind IP, WebRTC simulcast, hotkeys, virtual camera, multiview.
- **Streamlabs Desktop:** Dual Output — separate horizontal and vertical canvases streamed at once; per-destination orientation; add/remove destinations while live; Go Live window with per-platform title/category; themes/overlays; chat.
- **Church services (Resi, BoxCast):** resilient/ARQ upload that survives bad internet; website embed player; scheduling; replays; analytics. **StreamYard/vMix:** remote guests. **Restream:** multistream + merged chat.

## 3. Approach

Chosen: **reuse the 1.0 recording pipeline.** The app already renders screens with OpenGL/wgpu and feeds frames to an `ffmpeg` child (src/record). Livestreaming adds:

- a **stream canvas** (one wide, optionally one tall) rendered by our own compositor from scenes;
- an **ffmpeg child per canvas** encoding once (hardware encoder when present) and sending to all destinations of that canvas via the `tee` muxer with `onfail=ignore`, so one failing destination never stops the others; each destination can also get its own child when it needs different quality;
- a **sound engine** in Rust (cpal input capture + our mixer + filters) that sends PCM to ffmpeg over a pipe, with the app's own media sound already captured (src/record/sound.rs).

Rejected: linking libav in-process (new native deps and toolchain pain; Jake never wants MSVC); GStreamer (big new install).

FFmpeg is fetched like today (pinned, checksummed). NDI uses the NDI runtime if installed (dynamic load, optional).

## 4. Parts (each a module with one job)

1. **`stream::scene`** — the model: `Scene { id, name, layers }`, layers = camera, live slide (full / lower third / picture-in-picture), logo/prop, text, clock/countdown, image/video, browser source, colour; crop, position, chroma/luma key, opacity. Built-in scenes: *Camera*, *Camera + lyrics*, *Camera + verse*, *Full slide*, *Slide + camera PiP*, *Video*, *Starting soon* (countdown), *Ending*, *Hold* (still picture). Reuses Looks/Props/Masks/Themes from 1.0 for styling lower thirds.
2. **`stream::auto`** — chooses the scene from what is live: song → Camera + lyrics; Bible → Camera + verse; sermon/announcement text → Camera (or Camera + text if the slide has words); video → Video; presentation/designed slide → Full slide (or PiP, setting); nothing live/before service → Starting soon; after → Ending. Slide actions and macros (1.0) gain "Stream scene" and "Camera" steps that override it. A manual click or key takes over until the next item (setting: until next item / until next slide / stay manual).
3. **`stream::compositor`** — renders a scene to a canvas texture (wide 1920×1080 default; tall 1080×1920 optional with its own scene variants), on the existing render thread; feeds the encoder at a fixed fps; same frames feed recording, NDI out and the preview.
4. **`stream::cameras`** — sources: every OS video input (USB, capture cards), NDI, RTSP/SRT/HLS URLs, phone camera (the phone remote page opens the camera via WebRTC/WebSocket and sends frames). Health (signal lost → fallback picture, auto-return). PTZ: VISCA over IP and NDI PTZ, presets per scene. Switching: Program/Preview pair, Cut/Fade, number keys, phone tally.
5. **`stream::audio`** — sources: any input device (sound board via USB/line-in, mic), app sound, phone mic; per source: gain, mute, meter, delay (ms, to line up with the camera), noise suppression, gate, compressor, limiter, EQ (advanced); stream mix vs record tracks; monitor output; silence warning. Simple modes for small setups: "PC mic", "USB mic", "App sound only".
6. **`stream::outputs`** — destinations: YouTube, Facebook, Instagram, X, TikTok, Twitch (preset ingest URLs), Custom (RTMP/RTMPS/SRT URL + key). Each: name, on/off, canvas (wide/tall), quality profile (same as main / own). Add/remove while live. Speed test. Reconnect with backoff per destination. Dynamic bitrate (lower on congestion). Stream delay. SRT "safer send" mode with buffer. Bind network card.
7. **`stream::record`** — record the program (and optionally a clean feed without lyrics): format MP4 / MKV (+ auto-remux), name pattern, folder, quality = stream or own, split by time/size, chapter marks at items (song names, sermon), multi-track audio (mix + sound board raw), replay buffer ("save last N minutes"), mark-in/mark-out "sermon clip".
8. **`stream::accounts`** (sign-in, optional) — OAuth for YouTube (Live Streaming API), Facebook (Live Video API), Twitch (Helix). Creates/updates the broadcast: title, description, thumbnail, privacy, scheduled time; reads viewer count and chat; gives the replay link after. Tokens and stream keys in the Windows Credential Manager (keyring on Mac/Linux). Key-only destinations keep title/description in the app with a Copy button.
9. **`stream::ui`** — Go Live panel (button with countdown, health light, per-destination status, title/description editor with templates like "Sunday Service — {date}", scene buttons with live/next indication, camera buttons, audio meters, Hold/Mute panic buttons, chat/viewers when signed in); Stream settings (Easy page + "More" for Advanced); first-time wizard.
10. **`stream::ndi_out`** — NDI output of the program canvas, and a **key/fill** (alpha) lyrics feed for ATEM/vMix/OBS users; also alpha recording (MOV + QuickTime Animation / ProRes 4444 via ffmpeg) like ProPresenter.

## 5. Settings: easy vs advanced

**Easy (default page):** destinations (logos, paste key or Sign in, on/off, wide/tall), quality (Auto from speed test / Good 720p / Best 1080p), camera pick, sound pick (Sound board / USB mic / PC mic / App only), record on/off + folder, title/description template, auto scenes on/off.

**Advanced ("More"):**
- Video: canvas size, output size per destination, scale filter, fps (24/25/30/50/60).
- Encoder: auto / NVENC / QSV / AMF / x264; H.264 / HEVC / AV1 where the destination accepts it; rate control CBR/VBR/CQP; bitrate; keyframe interval (2 s default); preset; profile; B-frames.
- Audio: sample rate 44.1/48 kHz, bitrate, channels, per-source filters, delay, tracks.
- Network: dynamic bitrate on/off, stream delay seconds, reconnect delay and max tries, SRT latency, bind IP.
- Recording: everything in part 7.
- Scenes: full editor (layers, keys, crops, transitions incl. stinger video), hotkeys, multiview.
- Saved setups (presets) per service type; stats and logs after each stream.

## 6. When things go wrong

- **Before going live:** speed test; warns if upload is too slow for the chosen destinations and suggests a quality; checks keys/sign-ins; checks camera and sound are present.
- **Internet:** per-destination reconnect with backoff, others keep streaming; dynamic bitrate drop on congestion; SRT buffer mode; the health light turns yellow/red with plain words; recording continues regardless.
- **PC too slow:** watches dropped/late frames; plain advice ("Try 720p or the graphics card encoder"); falls back to a hardware encoder when available.
- **Camera unplugged:** that layer shows its fallback picture (logo/slide); returns when it's back. **Silence > 10 s:** warning.
- **Wrong key / expired sign-in:** clear message on that destination only.
- **Crash:** recording file stays safe (MKV/fragmented); on restart "You were live — go live again?".
- **Panic buttons:** Hold screen (still picture), Mute stream.

## 7. Testing

- Unit tests per module (scene choice from live state, layer layout, audio filters/meters/delay, output argument building, reconnect state machine, title templates).
- Fake ffmpeg (already used by record tests) and a local RTMP/SRT receiver for integration tests: multi-destination, one destination failing, reconnect, bitrate drop.
- Picture tests of scenes (lower thirds, PiP, tall layout) with the software renderer.
- Accounts tested against recorded API responses (no network in tests).
- On Jake's PC: a private/unlisted YouTube test stream with a camera before any Sunday use.

## 8. Build order (each step usable on its own)

1. **1.3a — Go Live basics:** stream any existing screen (ProPresenter-style) to many destinations with keys; Go Live panel, health light, reconnect, speed test, record at the same time; Easy output/recording settings.
2. **1.3b — Scenes:** stream canvas + built-in scenes + auto scenes + camera/lower third/PiP + starting-soon/ending/hold; slide actions/macros steps.
3. **1.3c — Sound:** mixer, meters, delay, filters, simple modes, tracks.
4. **1.3d — Cameras:** multiple cameras + switching (Program/Preview), NDI/RTSP/phone camera, PTZ presets, fallback pictures, tally.
5. **1.3e — Accounts:** YouTube/Facebook/Twitch sign-in, titles/descriptions/thumbnails/scheduling, chat + viewers, replay link.
6. **1.3f — Advanced:** scene editor, wide + tall at once, NDI out + key/fill + alpha recording, all Advanced settings, replay buffer, sermon clips, chapters, presets, stats.
Later: live captions (speech-to-text), remote guests, website embed player.

## 9. Open points for Jake

- Sign-in needs the app registered with Google (YouTube API verification) and Meta; stream keys work without it. Who owns the developer accounts? (Suggest: the church's Google/Meta account.)
- Instagram/TikTok only give stream keys to some accounts — the app will say so.
