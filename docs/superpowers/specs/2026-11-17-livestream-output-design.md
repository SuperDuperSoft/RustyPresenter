# Livestream outputs, encoding, recording and the before/after of a stream — design (November 17, 2026)

Branch `ls2-output`. Finishes what `2026-11-09-livestreaming-design.md` §4 items 6–7, §5 (Advanced) and §6 list for
destinations, encoding and recording. Everything new keeps what the app did before as its default, so a church that
never opens the new window sees no change.

## What already existed (checked before building)

YouTube, Facebook, Twitch and a custom RTMP/RTMPS/SRT server; reconnecting with waits; a lighter stream per
destination when its internet cannot keep up; the encoder and bitrate choice; a short summary after a stream; "the
computer is too slow" (without the graphics-card hint); a warning when a camera never sends; the words file (MOV,
lost in a crash); and "You were live — go live again?" after a crash (`record::unfinished`, asked by
`Operator::after_unfinished_recording`, called from `main.rs`). That last one needed nothing more.

## Destinations

- **Instagram, X, TikTok** join the list after the first four (`stream::BUILT_IN`, ids 5–7), so saved rows keep
  their places. Each shows a server address with its key; it can be typed (`Destination::url`) and wins over the
  usual one (Instagram `rtmps://live-upload.instagram.com:443/rtmp`, X `rtmps://va.pscp.tv:443/x`). TikTok has no
  address for everyone: it must be typed. Settings says plainly that these give keys only to some accounts.
- **Each destination's own quality** (`Destination::quality`: size and bitrate, default the main stream's). A
  destination that differs gets its picture encoded again by its own sender (`send::Way::own`), so the recording and
  the others are untouched; one that does not differ is copied as before. The lighter-stream ladder steps down from
  the destination's own bitrate.
- **SRT safer send**: `latency=` added to SRT addresses (0.5–4 s), unless the address already says.
- **Send from a network card**: the computer's cards are listed (`stream::network_card`, Windows' adapter list,
  `getifaddrs` elsewhere); RTMP and RTMPS senders bind to the chosen address (`-local_addr`). FFmpeg's SRT has no
  way to choose (checked on FFmpeg 9), and Settings says so.

## Encoding (More settings → Picture, Sound)

`record::advanced` makes FFmpeg's options; every default reproduces the old arguments exactly (tested).

- Pictures a second: 24, 25, 30 (usual), 50, 60. The writer, the cameras, NDI and the words file follow it.
- Video format: H.264, HEVC, AV1. Checked with FFmpeg 9 on this PC: FLV (enhanced RTMP) carries HEVC and AV1, and
  the stream's local feeds (MPEG-TS over UDP) carry HEVC, but **not AV1** (it comes back as unknown data). So: HEVC
  is used when every destination that is on takes it (YouTube and a custom server; `Service::accepts`), otherwise
  H.264 for all (no hidden second encodes); AV1 is used for recordings only, H.264 while live. The encoder comes
  from the same maker as the H.264 one (NVIDIA, Intel, AMD, Windows' own), then the processor's (x265, SVT-AV1),
  and only one that works on this computer.
- Bitrate kept: Automatic (as before), constant (CBR), varies (VBR, up to 1.5×), mapped to each encoder's options.
- A whole picture every 1–4 s (2 usual); encoder speed (x264/x265 presets, NVENC p1–p6, QSV presets, AMF quality,
  SVT-AV1 presets); profile (Baseline/Main/High; Baseline forces no B-frames); B-frames 0–3.
- Sound: 44.1/48 kHz, 96–320 kbit/s, stereo or mono.

## Recording (More settings → Recording)

- **MP4 or MKV**: written as MKV while recording, as before; MP4 is made at the end, or the MKV is kept as it is.
  HEVC in MP4 is tagged `hvc1` for Apple's players.
- **File name pattern**: `{date}`, `{time}`, `{title}` (the service's name); the default is the old name.
- **Cut long recordings**: every 30 min / 1 h / 2 h, or at 2/4/8 GB, done after the file is finished (copying, at
  whole pictures), so a crash or a failed cut never loses the recording: if cutting fails the whole file stays.
- **Clean recording** (while live): the same scenes without the words, put together by the writer beside the
  stream's picture (`Shown` with no words) and fed to a FFmpeg of its own (`stream::clean_file`) that never holds
  the stream up (pictures are left out there instead). Written as MKV, finished with the recording's sound.
- **Save the last N minutes**: a button beside Clip while recording (1–15 min, More settings); copies the last
  minutes out of the MKV being written, at once, into "… last 5 minutes.mp4". The app's own sound is added to
  recordings only at the end, so it is not in this copy (a sound input and the PC's sound are).
- **The words file survives a crash**: written as fragmented MOV (pieces of 1 s, QuickTime Animation keeps its
  see-through; MKV loses it, checked) and copied into an ordinary MOV at the end; one left by a crash is copied
  the next time a recording starts.

## Saved setups

`record::setups`: the whole livestream setup under a name (destinations on/off, cameras, sound, scenes, cards, More
settings). Not kept: stream keys and sign-ins (they stay in the password store), the broadcast made ahead, the
licence note, what was learnt about the encoder. Used, saved and deleted in the More window; the easy page shows
which setup the settings are.

## Before and after

- **Before going live** (`stream::preflight`): the cameras and the sound input chosen are looked for in the
  computer's lists and opened for a moment (one picture; half a second of sound), all at once, while the rest is
  got ready. What is missing is said in plain words; the stream still goes out.
- **After a stream** (`stream::report`): the summary as before, then, when there is more to say, the pictures the
  computer left out, the destinations that got a lighter stream (and how light), and the most viewers at once. All
  of it, per destination, goes to `logs/streams.log`.
- **Too slow**: the advice names 720p and the graphics card's encoder. When x264 cannot keep up and the encoder is
  left to the app, the app remembers it (`x264_too_slow`) and next time uses a graphics card's encoder that works.
  It does not change encoder in the middle of a stream: that would restart the recording into a second file and
  break every destination's connection for a moment, which is worse on a Sunday than a jerky picture.

## Tests

Timing-sensitive live tests now wait for what they look at (the camera's first picture, then a margin) instead of
fixed seconds (`run_tests::record_while`), with a 40 s limit so a real failure still fails.
